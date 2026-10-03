"use client";

import { createContext, useCallback, useContext, useEffect, useState } from "react";
import type { User } from "./types";

interface Session {
  token: string;
  user: User;
}

interface AuthValue {
  session: Session | null;
  ready: boolean;
  signIn: (s: Session) => void;
  signOut: () => void;
}

const KEY = "session";
const AuthContext = createContext<AuthValue | null>(null);

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [session, setSession] = useState<Session | null>(null);
  const [ready, setReady] = useState(false);

  // Restore after hydration so server and client render the same first paint.
  useEffect(() => {
    try {
      const raw = sessionStorage.getItem(KEY);
      if (raw) setSession(JSON.parse(raw));
    } catch {}
    setReady(true);
  }, []);

  const signIn = useCallback((s: Session) => {
    setSession(s);
    try {
      sessionStorage.setItem(KEY, JSON.stringify(s));
    } catch {}
  }, []);

  const signOut = useCallback(() => {
    setSession(null);
    try {
      sessionStorage.removeItem(KEY);
    } catch {}
  }, []);

  return (
    <AuthContext.Provider value={{ session, ready, signIn, signOut }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used inside AuthProvider");
  return ctx;
}
