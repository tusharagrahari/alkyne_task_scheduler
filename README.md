# Task Management API — Rust, Actix Web, PostgreSQL

A task-management backend with **email two-factor login**, **role-based access
control**, and **per-user caching** of the assigned-task view.

The primary validation point is `GET /tasks/view-my-tasks`: called as James Bond it
returns exactly his three assigned tasks with `cache.hit = false`, and the identical
second call returns `cache.hit = true`. The
[captured response](#final-validation-response) below is real output from the run
described in [Validation workflow](#validation-workflow).

---

## Contents

- [Stack](#stack)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [Migrations](#migrations)
- [Validation workflow](#validation-workflow)
- [Final validation response](#final-validation-response)
- [API reference](#api-reference)
- [Design notes](#design-notes)
- [Caching: design and limitations](#caching-design-and-limitations)
- [Tests](#tests)
- [Known limitations](#known-limitations)

---

## Stack

| Concern           | Choice                                                            |
| ----------------- | ----------------------------------------------------------------- |
| Language          | Rust 1.89 (edition 2024)                                          |
| Web framework     | Actix Web 4                                                       |
| Database          | PostgreSQL 17 via SQLx 0.8 (`runtime-tokio`, `tls-rustls`)        |
| Migrations        | `sqlx::migrate!` — embedded in the binary, applied on start-up     |
| Password / 2FA-code hashing | Argon2id (`argon2` 0.6)                                 |
| Tokens            | HS256 JWT (`jsonwebtoken` 11)                                     |
| Cache             | In-memory, per user, TTL'd — behind a `TaskCache` trait            |
| Mail              | `email_logs` table + `tracing`, readable via a dev endpoint        |
| Logging           | `tracing` / `tracing-subscriber` with `RUST_LOG` filtering         |
| Tests             | `cargo test` — unit tests plus HTTP integration tests             |

---

## Quick start

### Option A — everything in Docker (nothing but Docker required)

```bash
docker compose up --build
# API on http://localhost:8080, PostgreSQL on localhost:5432
```

The API waits for the database's health check, then applies migrations itself, so
the stack is ready as soon as `GET /health` answers:

```bash
curl -s localhost:8080/health
# {"status":"ok","database":"up","cached_task_lists":0}
```

### Option B — database in Docker, API on the host

```bash
docker compose up -d db          # PostgreSQL only
cp .env.example .env             # defaults already point at localhost:5432
cargo run                        # or: cargo run --release
```

`cargo run --release` is worth it if you are stepping through the flow by hand:
Argon2 is intentionally slow, and an unoptimised debug build makes each login
noticeably slower.

### Tear down

```bash
docker compose down -v           # -v also removes the database volume
```

---

## Configuration

Every setting is read from the environment once at start-up (see
[`src/config.rs`](src/config.rs)). `.env` is loaded if present; `docker compose`
passes the variables directly instead. The full list with defaults lives in
[`.env.example`](.env.example).

| Variable                      | Required | Default             | Purpose                                              |
| ----------------------------- | -------- | ------------------- | ---------------------------------------------------- |
| `DATABASE_URL`                | yes      | —                   | PostgreSQL connection URL                            |
| `JWT_SECRET`                  | yes      | —                   | HS256 signing key; **≥ 32 bytes or the app refuses to start** |
| `BIND_ADDRESS`                | no       | `127.0.0.1:8080`    | Listen address (`0.0.0.0:8080` in a container)        |
| `JWT_TTL_SECONDS`             | no       | `3600`              | Access-token lifetime                                |
| `TWO_FACTOR_CODE_TTL_SECONDS` | no       | `300`               | Verification-code lifetime (the required 5 minutes)   |
| `TWO_FACTOR_MAX_ATTEMPTS`     | no       | `5`                 | Wrong codes allowed per challenge before it locks     |
| `CACHE_TTL_SECONDS`           | no       | `300`               | Lifetime of a cached `view-my-tasks` payload          |
| `ENABLE_DEV_ENDPOINTS`        | no       | `true`              | Serves `/dev/email-logs*`, which reveal 2FA codes     |
| `SEED_*`                      | no       | see `.env.example`  | Credentials created by `POST /seed/users`             |
| `RUST_LOG`                    | no       | `info,sqlx::query=warn` | Log filter                                       |

---

## Migrations

The schema lives in [`migrations/`](migrations/) and is **embedded into the binary
at compile time**, so `cargo run` and `docker compose up` both apply it
automatically — there is no separate migration step to remember.

To drive migrations manually instead (requires `sqlx-cli`):

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
export DATABASE_URL=postgres://alkyne:alkyne@localhost:5432/alkyne
sqlx database create
sqlx migrate run
```

Schema summary — `users`, `tasks`, `login_challenges`, `email_logs`, with
PostgreSQL `ENUM` types `user_role`, `task_status`, `task_priority` so invalid
values are rejected by the database rather than by application validation alone.

---

## Validation workflow

The eleven steps from the assignment, as copy-pasteable `curl`. Uses
[`jq`](https://jqlang.github.io/jq/) to thread ids between calls. Run from a shell
with the API up.

```bash
API=http://localhost:8080
```

**1. Create the two users.** Idempotent — safe to re-run at any point.

```bash
curl -s -X POST $API/seed/users -H 'Content-Type: application/json' -d '{}' | jq
```

**2. Start the Admin login.** Returns a `login_challenge_id`, **not** a JWT.

```bash
ADMIN_CHALLENGE=$(curl -s -X POST $API/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"email":"admin@example.com","password":"AdminPass123!"}' \
  | jq -r .login_challenge_id)
echo "challenge: $ADMIN_CHALLENGE"
```

**3. Read the verification code** from the development email log. (It is also
printed to the server's stdout.)

```bash
ADMIN_CODE=$(curl -s "$API/dev/email-logs/latest?email=admin@example.com" \
  | jq -r .verification_code)
echo "code: $ADMIN_CODE"
```

**4. Verify the code and receive the Admin JWT.**

```bash
ADMIN_TOKEN=$(curl -s -X POST $API/auth/verify-2fa \
  -H 'Content-Type: application/json' \
  -d "{\"login_challenge_id\":\"$ADMIN_CHALLENGE\",\"code\":\"$ADMIN_CODE\"}" \
  | jq -r .access_token)
```

**5. Create exactly 5 tasks as Admin.**

```bash
TASK_IDS=()
while IFS='|' read -r title priority; do
  id=$(curl -s -X POST $API/tasks \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H 'Content-Type: application/json' \
    -d "{\"title\":\"$title\",\"description\":\"$title\",\"priority\":\"$priority\"}" \
    | jq -r .id)
  TASK_IDS+=("$id")
  echo "created [$priority] $id  $title"
done <<'TASKS'
Infiltrate the casino|high
Recover the stolen ledger|medium
File the expense report|low
Service the Aston Martin|medium
Brief Q on the new gadget|high
TASKS
```

**6. Assign exactly 3 of them to James Bond** — one `high`, one `medium`, one `low`.

```bash
curl -s -X POST $API/tasks/assign \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d "{\"task_ids\":[\"${TASK_IDS[0]}\",\"${TASK_IDS[1]}\",\"${TASK_IDS[2]}\"],
       \"assignee_email\":\"jamesbond@example.com\"}" | jq
```

**7 & 8. Log James Bond in through the same two-step flow.**

```bash
BOND_CHALLENGE=$(curl -s -X POST $API/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"email":"jamesbond@example.com","password":"BondPass123!"}' \
  | jq -r .login_challenge_id)

BOND_CODE=$(curl -s "$API/dev/email-logs/latest?email=jamesbond@example.com" \
  | jq -r .verification_code)

BOND_TOKEN=$(curl -s -X POST $API/auth/verify-2fa \
  -H 'Content-Type: application/json' \
  -d "{\"login_challenge_id\":\"$BOND_CHALLENGE\",\"code\":\"$BOND_CODE\"}" \
  | jq -r .access_token)
```

**9. Creating a task as James Bond must be `403 Forbidden`.**

```bash
curl -s -w '\nHTTP %{http_code}\n' -X POST $API/tasks \
  -H "Authorization: Bearer $BOND_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"title":"Promote myself","priority":"high"}'
```

```text
{"error":{"code":"insufficient_role","message":"this action requires the `admin` role"}}
HTTP 403
```

**10. `view-my-tasks` returns exactly 3 tasks with `cache.hit = false`.**

```bash
curl -s $API/tasks/view-my-tasks -H "Authorization: Bearer $BOND_TOKEN" | jq
```

**11. The identical call again returns `cache.hit = true`.**

```bash
curl -s $API/tasks/view-my-tasks -H "Authorization: Bearer $BOND_TOKEN" | jq .cache
# { "hit": true }
```

### Bonus: watch the cache invalidate

```bash
# Assign a fourth task...
curl -s -X POST $API/tasks/assign \
  -H "Authorization: Bearer $ADMIN_TOKEN" -H 'Content-Type: application/json' \
  -d "{\"task_ids\":[\"${TASK_IDS[3]}\"],\"assignee_email\":\"jamesbond@example.com\"}" >/dev/null

# ...and the next read is a miss again, now with four tasks.
curl -s $API/tasks/view-my-tasks -H "Authorization: Bearer $BOND_TOKEN" \
  | jq '{hit: .cache.hit, total: .summary.total_assigned_tasks}'
# { "hit": false, "total": 4 }
```

---

## Final validation response

Verbatim output of step 10 — `GET /tasks/view-my-tasks` with
`Authorization: Bearer <JAMES_BOND_TOKEN>` — captured by running the commands above
against a clean `docker compose up --build` stack:

```json
{
  "user": {
    "email": "jamesbond@example.com",
    "role": "staff"
  },
  "tasks": [
    {
      "id": "a747d06e-2708-4366-a564-caea03c2e936",
      "title": "Infiltrate the casino",
      "status": "todo",
      "priority": "high",
      "assigned_to": "jamesbond@example.com"
    },
    {
      "id": "0970252f-ae2f-4872-9437-93bc1af9509b",
      "title": "Recover the stolen ledger",
      "status": "todo",
      "priority": "medium",
      "assigned_to": "jamesbond@example.com"
    },
    {
      "id": "042cc2f7-00d3-4f3d-a158-ead875c28bef",
      "title": "File the expense report",
      "status": "todo",
      "priority": "low",
      "assigned_to": "jamesbond@example.com"
    }
  ],
  "summary": {
    "total_assigned_tasks": 3
  },
  "cache": {
    "hit": false
  }
}
```

Step 11 — the identical request, served from cache:

```json
{
  "user": { "email": "jamesbond@example.com", "role": "staff" },
  "tasks": [ "… identical to the three tasks above …" ],
  "summary": { "total_assigned_tasks": 3 },
  "cache": { "hit": true }
}
```

The task list is read from PostgreSQL with a join against `users`, not hardcoded:
see [`repo::tasks::list_assigned_to`](src/repo/tasks.rs).

---

## API reference

| Method  | Path                        | Auth        | Purpose                                                   |
| ------- | --------------------------- | ----------- | --------------------------------------------------------- |
| `GET`   | `/health`                   | public      | Liveness plus a real database round-trip                   |
| `POST`  | `/seed/users`               | public      | Create/refresh the Admin and James Bond accounts           |
| `POST`  | `/auth/login`               | public      | Verify password, open a 2FA challenge, "send" the code      |
| `POST`  | `/auth/verify-2fa`          | public      | Exchange the code for a JWT                                 |
| `GET`   | `/dev/email-logs/latest`    | dev-gated   | Latest "sent" email; `?email=` narrows by recipient         |
| `GET`   | `/dev/email-logs`           | dev-gated   | Recent emails; `?email=`, `?limit=` (max 100)               |
| `POST`  | `/tasks`                    | **admin**   | Create a task (optional `assignee_email` assigns at once)   |
| `GET`   | `/tasks`                    | **admin**   | All tasks — convenient for picking ids to assign            |
| `POST`  | `/tasks/assign`             | **admin**   | Assign tasks to a user, atomically                          |
| `PATCH` | `/tasks/{id}`               | **admin**   | Partial update — demonstrates invalidation on *update*      |
| `GET`   | `/tasks/view-my-tasks`      | any role    | The caller's own assigned tasks, with cache metadata        |

### Error format

Every failure returns the same envelope, so a client can branch on a stable code
rather than parsing prose:

```json
{ "error": { "code": "insufficient_role", "message": "this action requires the `admin` role" } }
```

That includes the framework's own extractor failures: malformed JSON, an unknown
field, and a non-UUID path parameter are all routed through the same envelope
rather than returning Actix's default plain-text body.

| Status | `error.code`                                                            |
| ------ | ----------------------------------------------------------------------- |
| 400    | `validation_error`                                                       |
| 401    | `invalid_credentials`, `missing_bearer_token`, `invalid_access_token`, `invalid_verification_code`, `verification_code_expired`, `verification_code_already_used` |
| 403    | `insufficient_role`, `dev_endpoint_disabled`                             |
| 404    | `not_found`                                                              |
| 409    | `conflict`                                                               |
| 429    | `too_many_verification_attempts`                                         |
| 500    | `internal_error` (details are logged, never returned)                    |

---

## Design notes

### Layering

```text
src/api/        HTTP: extractors, request/response DTOs, status codes
src/services/   use cases: 2FA login, admin-only writes, cache policy
src/repo/       every SQL statement in the application
src/domain/     entities and read models — no framework types
src/security/   Argon2, JWT, the auth extractors
src/cache/      TaskCache trait + in-memory implementation
src/mail/       outbound email (development transport)
```

Handlers decode, delegate, serialise. Business rules live in `services`, and every
SQL statement lives in `repo`, so the set of database operations is auditable in one
directory. Both the binary and the integration tests build the server through
`alkyne::build_app`, so the tests exercise the same routing table and middleware
that production serves.

### Two-factor authentication

`POST /auth/login` validates the password, then creates a `login_challenges` row
and "sends" an email. **It never returns a token.** `POST /auth/verify-2fa`
exchanges a correct code for a JWT.

- **Codes expire after 5 minutes** — `expires_at`, configurable.
- **Codes are single-use.** Enforced by the database, not by a read-then-write in
  application code: consumption is `UPDATE … WHERE consumed_at IS NULL RETURNING id`,
  so two requests racing with the same valid code cannot both be issued a token.
- **Codes are rate-limited** — 5 wrong attempts lock the challenge, and once locked
  even the correct code is refused.
- **Codes are not stored in plain text.** `login_challenges.code_hash` is an
  Argon2id PHC string. A six-digit code has only 10⁶ possible values, so a fast
  hash would be trivially reversible from a database dump; a memory-hard KDF is
  used for the code for the same reason it is used for passwords.
- **The `email_logs` row does contain the readable code** — that row *is* the
  delivered message, and the endpoint has to return a usable code for local
  validation. That is what `ENABLE_DEV_ENDPOINTS` gates.
- Unknown email and wrong password both return `invalid_credentials`, so accounts
  cannot be enumerated through the login endpoint.

### Role-based access control

Authorisation is expressed in handler *signatures* rather than in handler bodies:

```rust
#[post("")]                                     // POST /tasks
pub async fn create_task(state: web::Data<AppState>, admin: AdminUser, …)
```

`AdminUser` is a `FromRequest` extractor, so a non-admin never reaches the body.
The distinction the assignment asks for falls out of the ordering inside it:

| Situation                              | Result                        |
| -------------------------------------- | ----------------------------- |
| No / malformed `Authorization` header  | `401 missing_bearer_token`    |
| Bad or expired token                   | `401 invalid_access_token`    |
| Valid **staff** token, admin-only route | `403 insufficient_role`      |

`GET /tasks/view-my-tasks` filters on `assigned_to_id = <caller>`, so a staff user
physically cannot read another user's tasks — the restriction is in the query, not
in a post-filter.

The role travels inside the JWT, so authorising a request costs no database
round-trip. The trade-off: a role change only takes effect at the user's next
login. With a one-hour token that staleness window is acceptable; a deployment
needing instant revocation would add a token version or a short-lived
refresh-token exchange.

### Why runtime-checked SQLx queries

The code uses `sqlx::query_as` rather than the compile-time `query!` macros. The
macros verify SQL against a live database *while compiling*, which would make
`cargo build` depend on a reachable PostgreSQL or on a committed `.sqlx` cache that
must be regenerated on every schema change. Keeping the build hermetic matters more
here — a reviewer's first `cargo build` should never fail for want of a database —
and the integration tests execute every statement against a real PostgreSQL, so the
SQL is still verified, just at test time instead of build time.

---

## Caching: design and limitations

`GET /tasks/view-my-tasks` reads through a **per-user** cache keyed by user id
(`src/cache/`). The payload cached is exactly the payload served, so a hit and a
miss are byte-identical apart from `cache.hit`.

**One invariant, and it is the one the validation step checks:** only the read path
ever populates the cache. Creation, assignment and update *invalidate only* — they
never write through and never warm. If assignment populated the cache, step 10
would report `cache.hit = true` on the very first call and the feature would look
broken.

| Event                                         | Cache action                                        |
| --------------------------------------------- | --------------------------------------------------- |
| `GET /tasks/view-my-tasks` miss               | load from PostgreSQL, **insert**, report `hit=false` |
| `GET /tasks/view-my-tasks` hit                | serve from cache, report `hit=true`                  |
| `POST /tasks` with an `assignee_email`        | invalidate that assignee                             |
| `POST /tasks/assign`                          | invalidate the new assignee **and every previous assignee** |
| `PATCH /tasks/{id}`                           | invalidate the old and the new assignee              |
| `CACHE_TTL_SECONDS` elapses                   | entry expires lazily on the next read                |

Invalidation happens **after** the transaction commits, so a rolled-back assignment
does not cause a pointless reload. Reassignment invalidates *both* sides, which the
integration suite asserts directly.

### Redis was preferred; this uses an in-memory cache. The limitations:

1. **Process-local.** Entries live in this process's heap. Two API replicas keep
   independent caches, and an invalidation on one does **not** reach the other — a
   second replica could serve a stale list for up to `CACHE_TTL_SECONDS`. This
   implementation is correct for a single-process deployment only.
2. **Not durable.** A restart empties the cache. Harmless (the next read reloads
   from PostgreSQL) but it means hit rates start from zero after every deploy.
3. **Unbounded entry count.** Keys are user ids with no maximum size or LRU
   eviction, so memory grows with the number of *distinct users who have called the
   endpoint* within one TTL window. Fine at this scale; a real deployment wants a
   bounded cache.
4. **Lazy expiry.** No sweeper task: an expired entry is only evicted when it is
   next read, so an abandoned entry occupies memory until then.
5. **No cross-process coordination**, so no stampede protection — N concurrent
   misses for the same user all hit the database.

**The substitution point is explicit.** `AppState` holds
`Arc<dyn TaskCache>` — a four-method trait (`get` / `insert` / `invalidate` /
`entry_count`). Adding Redis means writing one more implementation of that trait
and changing one line in `main.rs`; limitations 1–5 all disappear and no service
code changes. That seam is the reason the trait exists rather than using the
concrete type directly.

---

## Tests

```bash
docker compose up -d db        # integration tests need a real PostgreSQL
cargo test
```

45 tests: 22 unit and 23 integration.

```text
test result: ok. 22 passed; 0 failed   (unit, src/)
test result: ok. 23 passed; 0 failed   (integration, tests/api_workflow.rs)
```

`DATABASE_URL` is honoured if set; otherwise the harness falls back to the
`docker-compose.yml` defaults.

**Isolation.** Every integration test creates its own database
(`alkyne_test_<uuid>`), migrates it, and drops it on success. That makes the suite
safe to run in parallel and lets each test assert absolute counts ("exactly 5
tasks", "exactly 3 assigned") without coordinating with its neighbours. A test that
*fails* deliberately leaves its database behind so the failing state can be
inspected — list leftovers with `psql -l | grep alkyne_test_`.

### Coverage against the assignment's "Testing Expectations"

| Expectation                                           | Test                                                        |
| ----------------------------------------------------- | ----------------------------------------------------------- |
| Admin and James Bond can be created                   | `seed_creates_admin_and_james_bond`, `seeding_twice_is_idempotent` |
| Login creates a 2FA challenge, returns no JWT          | `login_creates_challenge_and_does_not_return_a_jwt`         |
| Correct code returns a JWT                            | `correct_two_factor_code_returns_a_jwt`                     |
| Incorrect code rejected                               | `incorrect_two_factor_code_is_rejected_and_counted`         |
| Expired code rejected                                 | `expired_two_factor_code_is_rejected`                       |
| Reused code rejected                                  | `reused_two_factor_code_is_rejected`                        |
| Admin can create 5 tasks                              | `admin_can_create_five_tasks`                               |
| Admin can assign exactly 3 to James Bond              | `admin_can_assign_exactly_three_tasks_to_james_bond`        |
| James Bond cannot create a task                       | `james_bond_cannot_create_a_task`                           |
| James Bond sees exactly 3 assigned tasks              | `james_bond_sees_only_his_tasks_and_the_second_call_hits_the_cache` |
| `cache.hit` false then true                           | same test, plus `end_to_end_validation_workflow`            |
| Assignment / update invalidates the affected cache    | `assigning_another_task_invalidates_the_cached_view`, `updating_a_task_invalidates_the_cached_view`, `reassignment_invalidates_both_the_old_and_the_new_assignee` |

Beyond the required list: challenge attempt-limit locking, account
non-enumeration, `401` vs `403` ordering, atomic rollback when an assignment
contains an unknown task id, the development-endpoint gate, and
malformed-body handling (`400` on the same JSON envelope, not Actix's plain-text
default), and `end_to_end_validation_workflow`, which walks all eleven steps and
asserts the final response **field by field** — including that each task carries
exactly `id`, `title`, `status`, `priority`, `assigned_to` and nothing else.

### Lints

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Both are clean.

---

## Known limitations

Deliberate scope decisions, not oversights:

- **In-memory cache instead of Redis** — fully described in
  [Caching](#caching-design-and-limitations).
- **No real email delivery.** Messages go to `email_logs` and to stdout. Swapping in
  SMTP means one type with the same `send` signature (`src/mail/`).
- **`/dev/email-logs*` exposes verification codes** and must be off anywhere that
  is not a local machine: `ENABLE_DEV_ENDPOINTS=false` makes those routes `403`.
- **`POST /seed/users` is unauthenticated** and resets both passwords. Acceptable
  for a local validation fixture; it would be a CLI subcommand or a migration in a
  real service.
- **JWTs cannot be revoked before expiry**, and a role change takes effect at next
  login — see [Role-based access control](#role-based-access-control).
- **`PATCH /tasks/{id}` cannot clear a field.** Absent fields are expressed with
  `COALESCE`, so it can change `description` but not blank it, and can reassign a
  task but not unassign it.
- **Migrations run on start-up.** Convenient for one process; a multi-replica
  deployment should run them as a separate release step. SQLx takes an advisory
  lock, so concurrent replicas would still be correct — just serialised.
- **No rate limiting on `/auth/login`.** Per-challenge attempts are capped, but
  nothing caps challenge *creation*.

---

## AI usage

See [AI_USAGE.md](AI_USAGE.md).
