"use client";

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import type { Task } from "@/lib/types";
import { CreateTaskForm } from "./CreateTaskForm";
import { Notice } from "./Notice";

const ASSIGNEE = "jamesbond@example.com";

export function AdminPanel({ token }: { token: string }) {
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);
  const [assigning, setAssigning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setTasks(await api.listTasks(token));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load tasks");
    } finally {
      setLoading(false);
    }
  }, [token]);

  useEffect(() => {
    load();
  }, [load]);

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function assign() {
    setAssigning(true);
    setError(null);
    setSuccess(null);
    try {
      const r = await api.assignTasks(token, [...selected], ASSIGNEE);
      setSuccess(`Assigned ${r.assigned_count} task(s) to ${ASSIGNEE}`);
      setSelected(new Set());
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Assignment failed");
    } finally {
      setAssigning(false);
    }
  }

  return (
    <>
      <CreateTaskForm token={token} onCreated={load} />
      <section className="card">
        <h2>All tasks</h2>
        {loading && <p>Loading tasks…</p>}
        {error && <Notice kind="error">{error}</Notice>}
        {success && <Notice kind="success">{success}</Notice>}
        {!loading && tasks?.length === 0 && <p className="muted">No tasks yet. Create one above.</p>}
        {tasks && tasks.length > 0 && (
          <>
            <ul className="tasks">
              {tasks.map((t) => (
                <li key={t.id}>
                  <label className="row">
                    <input type="checkbox" checked={selected.has(t.id)} onChange={() => toggle(t.id)} />
                    <span className="grow">{t.title}</span>
                    <span className={`tag ${t.priority}`}>{t.priority}</span>
                    <span className="tag">{t.assigned_to_id ? "assigned" : "unassigned"}</span>
                  </label>
                </li>
              ))}
            </ul>
            <button disabled={selected.size === 0 || assigning} onClick={assign}>
              {assigning ? "Assigning…" : `Assign ${selected.size} selected to James Bond`}
            </button>
          </>
        )}
      </section>
    </>
  );
}
