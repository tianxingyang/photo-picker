import { useState } from "react";
import { echo } from "./api/echo";

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
      setReply(`error: ${String(e)}`);
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
