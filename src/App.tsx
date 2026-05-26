import { useState } from "react";

import { analyzePending } from "./api/analysisApi";
import { pickFolder } from "./api/dialogApi";
import { groupPhotos } from "./api/groupsApi";
import { scanFolder } from "./api/photosApi";
import { GroupBrowseView } from "./components/browse";
import { useGroupsStore } from "./store/groupsStore";
import { usePhotosStore } from "./store/photosStore";
import { describeAppError } from "./types/ipc";
import type { PhotoId } from "./types/photo";

const BTN =
  "rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary";

export function App() {
  const importedCount = usePhotosStore((s) => s.order.length);
  const addPhotos = usePhotosStore((s) => s.addPhotos);
  const loadGroups = useGroupsStore((s) => s.load);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  async function onImport() {
    if (busy) return;
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      const folder = await pickFolder();
      if (folder === null) return;
      const { photos, skipped } = await scanFolder(folder);
      addPhotos(photos);
      // why: hydrate the browse grid right after import so freshly-imported
      // (not-yet-analysed) photos show in the "未分组" bucket immediately —
      // analysis/grouping then just reshuffles them into similar groups.
      await loadGroups();
      if (skipped > 0) setNotice(`${skipped} 项无法读取，已跳过`);
    } catch (e) {
      const { kind, message } = describeAppError(e);
      setError(kind === "NotFound" ? `目录无效：${message}` : `导入失败：${message}`);
    } finally {
      setBusy(false);
    }
  }

  async function onAnalyzeAndGroup() {
    if (busy) return;
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      await analyzePending();
      await groupPhotos();
      await loadGroups();
    } catch (e) {
      const { message } = describeAppError(e);
      setError(`分析/分组失败：${message}`);
    } finally {
      setBusy(false);
    }
  }

  // Mount point for the A/B compare viewer — landed by the ab-compare task.
  function onCompare(_id: PhotoId) {
    setNotice("A/B 对比将在后续任务（ab-compare）实现");
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex flex-wrap items-center gap-3 border-b border-border px-4 py-3">
        <h1 className="text-base font-semibold">Photo Picker</h1>
        <button type="button" onClick={onImport} disabled={busy} className={BTN}>
          导入文件夹
        </button>
        <button type="button" onClick={onAnalyzeAndGroup} disabled={busy} className={BTN}>
          {busy ? "处理中…" : "分析并分组"}
        </button>
        <span className="text-xs text-muted-foreground">已导入 {importedCount} 张</span>
      </header>
      {(error ?? notice) !== null && (
        <div className="border-b border-border px-4 py-2 text-sm text-muted-foreground">
          {error ?? notice}
        </div>
      )}
      <div className="min-h-0 flex-1">
        <GroupBrowseView onCompare={onCompare} />
      </div>
    </div>
  );
}
