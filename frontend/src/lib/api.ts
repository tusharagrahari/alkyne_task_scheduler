import type {
  AccessToken,
  CreateTaskInput,
  EmailLog,
  LoginChallenge,
  MyTasks,
  SeededCredential,
  Task,
} from "./types";

const BASE = "/api"; // proxied to the Rust API by next.config.mjs

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
    public code?: string,
  ) {
    super(message);
  }
}

async function request<T>(
  path: string,
  opts: { method?: string; body?: unknown; token?: string } = {},
): Promise<T> {
  const headers: Record<string, string> = {};
  if (opts.body !== undefined) headers["Content-Type"] = "application/json";
  if (opts.token) headers.Authorization = `Bearer ${opts.token}`;

  let res: Response;
  try {
    res = await fetch(`${BASE}${path}`, {
      method: opts.method ?? "GET",
      headers,
      body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
    });
  } catch {
    throw new ApiError(0, "Cannot reach the API. Is the Rust server running?");
  }

  const text = await res.text();
  const data = text ? JSON.parse(text) : null;
  if (!res.ok) {
    // Backend error shape: { error: { code, message } }
    throw new ApiError(
      res.status,
      data?.error?.message ?? `Request failed (${res.status})`,
      data?.error?.code,
    );
  }
  return data as T;
}

export const api = {
  seedUsers: () =>
    request<{ credentials: SeededCredential[] }>("/seed/users", {
      method: "POST",
      body: {},
    }),
  login: (email: string, password: string) =>
    request<LoginChallenge>("/auth/login", {
      method: "POST",
      body: { email, password },
    }),
  verifyTwoFactor: (login_challenge_id: string, code: string) =>
    request<AccessToken>("/auth/verify-2fa", {
      method: "POST",
      body: { login_challenge_id, code },
    }),
  latestEmailLog: (email: string) =>
    request<EmailLog>(`/dev/email-logs/latest?email=${encodeURIComponent(email)}`),
  listTasks: (token: string) =>
    request<{ tasks: Task[] }>("/tasks", { token }).then((r) => r.tasks),
  createTask: (token: string, input: CreateTaskInput) =>
    request<Task>("/tasks", { method: "POST", body: input, token }),
  assignTasks: (token: string, task_ids: string[], assignee_email: string) =>
    request<{ assigned_count: number }>("/tasks/assign", {
      method: "POST",
      body: { task_ids, assignee_email },
      token,
    }),
  viewMyTasks: (token: string) => request<MyTasks>("/tasks/view-my-tasks", { token }),
};
