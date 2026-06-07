import { useEffect, useState } from "react";

import { analyzePending } from "./api/analysisApi";
import { pickFolder } from "./api/dialogApi";
import { groupPhotos } from "./api/groupsApi";
import { exportKeep, scanFolder } from "./api/photosApi";
import { ABCompareViewer } from "./components/compare";
import { GroupBrowseView } from "./components/browse";
import { clearDisplayCache } from "./hooks/useDisplaySrc";
import { useGroupsStore } from "./store/groupsStore";
import { usePhotosStore } from "./store/photosStore";
import { useCompareStore } from "./store/compareStore";
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

  // why: hydrate the browse grid on mount so photos from a prior session
  // (persisted in SQLite) show immediately, not only after a re-import.
  // loadGroups is a stable zustand action, so this runs once.
  useEffect(() => {
    loadGroups().catch((e) => {
      const { message } = describeAppError(e);
      setError(`加载照片失败：${message}`);
    });
  }, [loadGroups]);

  async function onImport() {
    if (busy) return;
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      const folder = await pickFolder();
      if (folder === null) return;
      const { photos, skipped } = await scanFolder(folder);
      // why: a re-scan may have replaced files at paths whose (path-derived)
      // PhotoId is unchanged; drop cached transcode URLs so HEIC display
      // re-resolves through the backend, which re-reads the live file mtime.
      clearDisplayCache();
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

  async function onExport() {
    if (busy) return;
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      const destDir = await pickFolder();
      if (destDir === null) return; // 用户取消选择目录
      const { exported, renamed, failed } = await exportKeep(destDir);
      if (exported === 0 && failed.length === 0) {
        setNotice("没有标记为「保留」的照片，未导出任何文件");
      } else {
        const parts = [`已导出 ${exported} 张`];
        if (renamed > 0) parts.push(`${renamed} 张因重名改名`);
        if (failed.length > 0) parts.push(`${failed.length} 项失败`);
        setNotice(parts.join("，"));
      }
    } catch (e) {
      const { message } = describeAppError(e);
      setError(`导出失败：${message}`);
    } finally {
      setBusy(false);
    }
  }

  const openFor = useCompareStore((s) => s.openFor);

  function onCompare(id: PhotoId) {
    openFor(id);
  }

  return (
    <div className="flex h-full flex-col">
      {/* A/B compare overlay — rendered at App level so it covers the full viewport. */}
      <ABCompareViewer />
      <header className="flex flex-wrap items-center gap-3 border-b border-border px-4 py-3">
        <h1 className="text-base font-semibold">Photo Picker</h1>
        <button type="button" onClick={onImport} disabled={busy} className={BTN}>
          导入文件夹
        </button>
        <button type="button" onClick={onAnalyzeAndGroup} disabled={busy} className={BTN}>
          {busy ? "处理中…" : "分析并分组"}
        </button>
        <button type="button" onClick={onExport} disabled={busy} className={BTN}>
          导出精选
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
