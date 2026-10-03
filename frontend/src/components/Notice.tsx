export function Notice({ kind, children }: { kind: "error" | "success" | "info"; children: React.ReactNode }) {
  return (
    <p className={`notice ${kind}`} role={kind === "error" ? "alert" : "status"}>
      {children}
    </p>
  );
}
