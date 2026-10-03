"use client";

import { useState } from "react";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import type { LoginChallenge } from "@/lib/types";
import { Notice } from "./Notice";

export function LoginForm() {
  const { signIn } = useAuth();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [challenge, setChallenge] = useState<LoginChallenge | null>(null);
  const [code, setCode] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);

  async function run(fn: () => Promise<void>) {
    setLoading(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Something went wrong");
    } finally {
      setLoading(false);
    }
  }

  const seed = () =>
    run(async () => {
      const r = await api.seedUsers();
      setInfo(r.credentials.map((c) => `${c.email} / ${c.password}`).join("  •  "));
    });

  const startLogin = (e: React.FormEvent) => {
    e.preventDefault();
    return run(async () => {
      setInfo(null);
      setChallenge(await api.login(email, password));
    });
  };

  const verify = (e: React.FormEvent) => {
    e.preventDefault();
    if (!challenge) return;
    return run(async () => {
      const r = await api.verifyTwoFactor(challenge.login_challenge_id, code.trim());
      signIn({ token: r.access_token, user: r.user });
    });
  };

  // Dev-only convenience: the code is "emailed" to the dev email log.
  const fetchCode = () =>
    run(async () => {
      const log = await api.latestEmailLog(email);
      if (!log.verification_code) throw new Error("No verification code found");
      setCode(log.verification_code);
    });

  const reset = () => {
    setChallenge(null);
    setCode("");
    setError(null);
  };

  return (
    <section className="card">
      {!challenge ? (
        <form onSubmit={startLogin}>
          <h2>Sign in</h2>
          <label>
            Email
            <input type="email" required value={email} onChange={(e) => setEmail(e.target.value)} />
          </label>
          <label>
            Password
            <input type="password" required value={password} onChange={(e) => setPassword(e.target.value)} />
          </label>
          <button disabled={loading}>{loading ? "Signing in…" : "Continue"}</button>
          <button type="button" className="link" disabled={loading} onClick={seed}>
            Seed Admin &amp; James Bond users
          </button>
        </form>
      ) : (
        <form onSubmit={verify}>
          <h2>Two-factor verification</h2>
          <Notice kind="info">{challenge.message}</Notice>
          <label>
            Verification code
            <input
              inputMode="numeric"
              required
              autoFocus
              value={code}
              onChange={(e) => setCode(e.target.value)}
            />
          </label>
          <button disabled={loading}>{loading ? "Verifying…" : "Verify"}</button>
          <button type="button" className="link" disabled={loading} onClick={fetchCode}>
            Dev: fill code from email log
          </button>
          <button type="button" className="link" onClick={reset}>
            Back
          </button>
        </form>
      )}
      {error && <Notice kind="error">{error}</Notice>}
      {info && <Notice kind="success">Users ready: {info}</Notice>}
    </section>
  );
}
