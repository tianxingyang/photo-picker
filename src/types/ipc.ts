// Mirrors the Rust `AppError` enum (src-tauri/src/error.rs). Tauri serializes
// it as { kind, message }. The frontend switches on `kind`, never on message.
export type AppErrorPayload =
  | { kind: "Sidecar"; message: string }
  | { kind: "Db"; message: string }
  | { kind: "Io"; message: string }
  | { kind: "NotFound"; message: string };

export type AppErrorKind = AppErrorPayload["kind"];

const KINDS: readonly string[] = ["Sidecar", "Db", "Io", "NotFound"];

// Normalize an unknown thrown value into a kind (when it matches the Rust
// contract) plus a human-readable message.
export function describeAppError(e: unknown): { kind?: AppErrorKind; message: string } {
  if (e && typeof e === "object" && "kind" in e && "message" in e) {
    const kind = (e as { kind: unknown }).kind;
    const message = (e as { message: unknown }).message;
    if (typeof kind === "string" && KINDS.includes(kind) && typeof message === "string") {
      return { kind: kind as AppErrorKind, message };
    }
  }
  if (typeof e === "string") return { message: e };
  // why: a plain Error has a non-enumerable `message`, so JSON.stringify(e)
  // would yield "{}" and hide the real text (e.g. boundary validation errors).
  if (e instanceof Error) return { message: e.message };
  try {
    return { message: JSON.stringify(e) };
  } catch {
    return { message: String(e) };
  }
}
