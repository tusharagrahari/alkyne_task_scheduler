export type Role = "admin" | "staff";
export type TaskStatus = "todo" | "in_progress" | "done";
export type TaskPriority = "low" | "medium" | "high";

export interface User {
  id: string;
  full_name: string;
  email: string;
  role: Role;
}

export interface LoginChallenge {
  login_challenge_id: string;
  expires_at: string;
  message: string;
}

export interface AccessToken {
  access_token: string;
  expires_at: string;
  user: User;
}

export interface Task {
  id: string;
  title: string;
  description: string | null;
  status: TaskStatus;
  priority: TaskPriority;
  assigned_to_id: string | null;
}

export interface AssignedTask {
  id: string;
  title: string;
  status: TaskStatus;
  priority: TaskPriority;
  assigned_to: string;
}

export interface MyTasks {
  user: { email: string; role: Role };
  tasks: AssignedTask[];
  summary: { total_assigned_tasks: number };
  cache: { hit: boolean };
}

export interface EmailLog {
  verification_code: string | null;
  login_challenge_id: string | null;
}

export interface SeededCredential {
  email: string;
  password: string;
  role: Role;
}

export interface CreateTaskInput {
  title: string;
  description?: string;
  priority: TaskPriority;
}
