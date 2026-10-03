"use client";

import { useState } from "react";
import { api, ApiError } from "@/lib/api";
import type { TaskPriority } from "@/lib/types";
import { Notice } from "./Notice";

export function CreateTaskForm({ token, onCreated }: { token: string; onCreated?: () => void }) {
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [priority, setPriority] = useState<TaskPriority>("medium");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError(null);
    setSuccess(null);
    try {
      const task = await api.createTask(token, {
        title,
        description: description || undefined,
        priority,
      });
      setSuccess(`Created "${task.title}"`);
      setTitle("");
      setDescription("");
      onCreated?.();
    } catch (err) {
      if (err instanceof ApiError && err.status === 403) {
        setError("403 Forbidden: only an admin can create tasks. Your account is not allowed to do this.");
      } else {
        setError(err instanceof Error ? err.message : "Something went wrong");
      }
    } finally {
      setLoading(false);
    }
  }

  return (
    <form className="card" onSubmit={submit}>
      <h2>Create task</h2>
      <label>
        Title
        <input required value={title} onChange={(e) => setTitle(e.target.value)} />
      </label>
      <label>
        Description
        <input value={description} onChange={(e) => setDescription(e.target.value)} />
      </label>
      <label>
        Priority
        <select value={priority} onChange={(e) => setPriority(e.target.value as TaskPriority)}>
          <option value="high">high</option>
          <option value="medium">medium</option>
          <option value="low">low</option>
        </select>
      </label>
      <button disabled={loading}>{loading ? "Creating…" : "Create task"}</button>
      {error && <Notice kind="error">{error}</Notice>}
      {success && <Notice kind="success">{success}</Notice>}
    </form>
  );
}
