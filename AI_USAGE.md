# AI Usage

The assignment states that AI tools are allowed provided the submitted code is
understood and explainable. This file records what was generated with assistance,
what was decided by hand, and what had to be corrected.

## Tool

**Claude Code (Opus 5)**, run from the terminal in this repository. No other AI
tooling (no Copilot, no ChatGPT) was used.

## Decisions made by me, not the model

These were chosen before any code existed, and the model was told to build to them:

- **Actix Web** for the HTTP layer (over Axum).
- **PostgreSQL + SQLx + docker-compose** (over the simpler SQLite path the spec
  also allows), because the spec states Postgres is preferred.
- **In-memory caching** rather than Redis, with the limitations documented — the
  spec permits this if it is written down.
- **Scope**: Docker Compose in, OpenAPI/Swagger UI and GitHub Actions CI out.
- **Local commits only**; nothing pushed to a remote automatically.

## What the model did

- Extracted the requirements from the supplied PDF and restated them as a checklist
  before writing anything.
- Produced the first draft of the whole tree: `migrations/0001`-equivalent schema,
  the `domain` / `repo` / `services` / `api` layering, `security` (Argon2, JWT,
  extractors), `cache`, `mail`, the Dockerfile and compose file, the test harness,
  and this documentation.
- Wrote the 45 tests.
- Ran `cargo fmt`, `cargo clippy -- -D warnings`, the test suite, and the eleven-step
  `curl` flow against a live server, iterating until all were green.

## Corrections and non-obvious work during the build

Not a clean one-shot generation. The things that actually needed fixing:

1. **Crate APIs had moved.** The latest releases of three dependencies differ from
   the versions the model first assumed, so a throwaway probe binary was compiled
   to pin down real signatures before writing any application code:
   - `argon2` 0.6 restructured its features (no `std`; `alloc` + `getrandom` +
     `password-hash` are the defaults) and `hash_password` now generates its own
     salt instead of taking a `SaltString`.
   - `rand` 0.10 moved `random_range` onto a new `RngExt` trait.
   - `jsonwebtoken` 11 **panics at runtime** unless a crypto provider feature is
     selected; the build pins `default-features = false, features = ["rust_crypto"]`
     so the Docker image needs no C toolchain.
2. **`assigned_to` is an email, not a UUID.** The data model the spec describes has
   `assigned_to_id`, but the expected response shows an email address. The
   `view-my-tasks` read model joins `users` to produce it. This is the single
   easiest way to fail the validation point and it was caught by review, not by the
   compiler.
3. **A dedicated DTO for the validation response.** `AssignedTask` carries exactly
   the five specified fields. Reusing the general task DTO would have leaked
   `description` and timestamps into the response the reviewer diffs.
4. **Enum case has to agree in three independent places** — the PostgreSQL `CREATE
   TYPE` labels, `#[sqlx(rename_all)]`, and `#[serde(rename_all)]`. All three are
   `snake_case`; a mismatch shows up as either a decode error or wrong-case JSON.
5. **The cache write-path invariant.** Only the read path may insert. An earlier
   plan had assignment warming the cache, which would have made step 10 report
   `cache.hit = true` and the headline feature look broken. It is now a documented
   invariant on the `TaskCache` trait with a test asserting `entry_count() == 0`
   after assignment.
6. **Test isolation was redesigned.** The first approach gave each test its own
   PostgreSQL *schema* via `SET search_path`. That has sharp edges — a `search_path`
   pointing at a missing schema fails silently, and per-schema enum types interact
   badly with SQLx's connection-level type caching. Replaced with a real database
   per test, dropped with `WITH (FORCE)` after the pool closes.
7. **Cache TTL default raised to 300s.** A short TTL would expire between a human's
   step 10 and step 11 and look like a broken cache.
8. **`Logger` middleware changed the response body type**, so `build_app`'s return
   type needed `ServiceResponse<impl MessageBody>` rather than the default
   `ServiceResponse`.
9. **`use actix_web::test` shadows the built-in `#[test]` attribute** (actix-web
   exports a proc-macro of the same name), which made one synchronous unit test fail
   to compile with a confusing error.
10. **One test assertion was simply wrong**: it compared JSON object key *order*,
    but `serde_json::Value` stores keys in a map and re-sorts them on parse. The
    assertion now compares key sets; wire order was verified separately with `curl`,
    and it does match the spec (`user`, `tasks`, `summary`, `cache`).
11. **Actix's extractor errors bypassed the error envelope.** A malformed JSON body
    returned a plain-text `400`, not the documented `{"error":{"code",…}}` shape.
    Found by probing the running server, not by the tests — `JsonConfig`,
    `QueryConfig` and `PathConfig` now each carry an `error_handler` that maps to
    `AppError`, with a test covering all three.
12. **`.env.example` needed quoting.** `SEED_STAFF_FULL_NAME=James Bond` parses fine
    through `dotenvy` but breaks a shell `source`, so the value is quoted.

## Verification — what I actually ran, not just read

- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test`: 45/45 passing against a real PostgreSQL 17 container.
- The full eleven-step flow by `curl` against a release build, with the resulting
  `view-my-tasks` body pasted verbatim into the README.
- Cache behaviour checked live and not only in tests: `hit=false` → `hit=true`,
  `hit=false` again after assigning a fourth task, and again after a `PATCH`.
- Reuse of a consumed 2FA code confirmed to return
  `401 verification_code_already_used` against the running server.
- `docker compose build` exercised so the shipped Dockerfile is known to compile.

## What I can explain

Every design decision above, plus specifically: why single-use 2FA is enforced with
`UPDATE … WHERE consumed_at IS NULL RETURNING id` instead of a read-then-write; why
the 2FA code is Argon2-hashed rather than SHA-256'd (10⁶ keyspace); why Argon2 runs
inside `web::block`; why authorisation lives in a `FromRequest` extractor rather
than in handler bodies, and how that produces `401` versus `403`; why assignment is
one transaction with `SELECT … FOR UPDATE`; why cache invalidation happens after
commit and covers the previous assignee; and the full cost of choosing an in-memory
cache over Redis.
