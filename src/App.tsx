import { useState } from "react";

import { pickFolder } from "./api/dialogApi";
import { scanFolder } from "./api/photosApi";
import { usePhotosStore } from "./store/photosStore";
import { describeAppError } from "./types/ipc";

export function App() {
  const order = usePhotosStore((s) => s.order);
  const byId = usePhotosStore((s) => s.byId);
  const addPhotos = usePhotosStore((s) => s.addPhotos);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  async function onImport() {
    // why: set busy before the (slow) picker await so a second click can't
    // open a second dialog and launch a concurrent scan; finally always resets.
    if (busy) return;
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      const folder = await pickFolder();
      if (folder === null) return;
      const { photos, skipped } = await scanFolder(folder);
      addPhotos(photos);
      if (skipped > 0) setNotice(`${skipped} 项无法读取，已跳过`);
    } catch (e) {
      const { kind, message } = describeAppError(e);
      setError(kind === "NotFound" ? `目录无效：${message}` : `导入失败：${message}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <main>
      <h1>Photo Picker</h1>
      <div className="row">
        <button onClick={onImport} disabled={busy}>
          {busy ? "导入中…" : "导入文件夹"}
        </button>
        <span>已导入 {order.length} 张</span>
      </div>
      {error !== null && <p className="reply">{error}</p>}
      {notice !== null && <p className="reply">{notice}</p>}
      <ul className="photo-list">
        {order.map((id) => {
          const p = byId[id];
          return p ? <li key={id}>{p.name}</li> : null;
        })}
      </ul>
    </main>
  );
}
