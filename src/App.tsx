import { useState } from "react";
import { echo } from "./api/echo";

// why: Tauri rejects invoke() with the serialized Rust error object
// ({kind, message} for AppError), so String(e) yields "[object Object]".
function describeError(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) {
    const m = (e as { message: unknown }).message;
    if (typeof m === "string") return m;
  }
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

export function App() {
  const [text, setText] = useState("hello sidecar");
  const [reply, setReply] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onEcho() {
    setBusy(true);
    try {
      const r = await echo(text);
      setReply(`sidecar replied: ${r}`);
    } catch (e) {
      setReply(`error: ${describeError(e)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <main>
      <h1>Photo Picker — M0</h1>
      <p>Rust ↔ Python sidecar echo test.</p>
      <div className="row">
        <input value={text} onChange={(e) => setText(e.target.value)} />
        <button onClick={onEcho} disabled={busy}>
          {busy ? "..." : "Echo"}
        </button>
      </div>
      {reply !== null && <p className="reply">{reply}</p>}
    </main>
  );
}
