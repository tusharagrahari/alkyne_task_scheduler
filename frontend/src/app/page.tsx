"use client";

import { AdminPanel } from "@/components/AdminPanel";
import { LoginForm } from "@/components/LoginForm";
import { StaffTasks } from "@/components/StaffTasks";
import { useAuth } from "@/lib/auth";

export default function Home() {
  const { session, ready, signOut } = useAuth();

  return (
    <main>
      <header>
        <h1>Task Manager</h1>
        {session && (
          <div className="who">
            {session.user.full_name} ({session.user.role})
            <button className="link" onClick={signOut}>
              Sign out
            </button>
          </div>
        )}
      </header>
      {!ready ? null : !session ? (
        <LoginForm />
      ) : session.user.role === "admin" ? (
        <AdminPanel token={session.token} />
      ) : (
        <StaffTasks token={session.token} />
      )}
    </main>
  );
}
