"use client";

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import type { MyTasks } from "@/lib/types";
import { CreateTaskForm } from "./CreateTaskForm";
import { Notice } from "./Notice";

export function StaffTasks({ token }: { token: string }) {
  const [data, setData] = useState<MyTasks | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setData(await api.viewMyTasks(token));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load tasks");
    } finally {
      setLoading(false);
    }
  }, [token]);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <>
      <section className="card">
        <h2>My assigned tasks</h2>
        {loading && <p>Loading tasks…</p>}
        {error && <Notice kind="error">{error}</Notice>}
        {data && (
          <>
            <p className="muted">
              {data.summary.total_assigned_tasks} assigned · cache.hit ={" "}
              <strong>{String(data.cache.hit)}</strong>
            </p>
            {data.tasks.length === 0 ? (
              <p className="muted">Nothing is assigned to you yet.</p>
            ) : (
              <ul className="tasks">
                {data.tasks.map((t) => (
                  <li key={t.id} className="row">
                    <span className="grow">{t.title}</span>
                    <span className={`tag ${t.priority}`}>{t.priority}</span>
                    <span className="tag">{t.status}</span>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
        <button disabled={loading} onClick={load}>
          Refresh
        </button>
      </section>
      {/* Staff cannot create tasks; this exists to demonstrate the 403 handling. */}
      <CreateTaskForm token={token} />
    </>
  );
}
