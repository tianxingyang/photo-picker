import { useEffect, useState } from "react";

import { analyzePending } from "./api/analysisApi";
import { pickFolder } from "./api/dialogApi";
import { groupPhotos } from "./api/groupsApi";
import { basename, exportKeep, scanFolder, type ExportFailure } from "./api/photosApi";
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
  const [failures, setFailures] = useState<ExportFailure[]>([]);

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
    setFailures([]);
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
    setFailures([]);
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
    setFailures([]);
    setBusy(true);
    try {
      const destDir = await pickFolder();
      if (destDir === null) return; // 用户取消选择目录
      const { exported, renamed, skipped, failed } = await exportKeep(destDir);
      setFailures(failed);
      if (exported === 0 && skipped === 0 && failed.length === 0) {
        setNotice("没有标记为「保留」的照片，未导出任何文件");
      } else {
        const parts = [`已导出 ${exported} 张`];
        if (renamed > 0) parts.push(`${renamed} 张因重名改名`);
        if (skipped > 0) parts.push(`${skipped} 张已在目标文件夹中，已跳过`);
        // keep the failure COUNT in the announced notice (the <details> below
        // carries the per-item breakdown); otherwise an all-failures export would
        // read a misleading "已导出 0 张" with no spoken failure signal.
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
      {error !== null && (
        <div role="alert" className="border-b border-border px-4 py-2 text-sm text-muted-foreground">
          {error}
        </div>
      )}
      {error === null && notice !== null && (
        <div
          role="status"
          aria-live="polite"
          className="border-b border-border px-4 py-2 text-sm text-muted-foreground"
        >
          {notice}
        </div>
      )}
      {error === null && failures.length > 0 && (
        <details className="border-b border-border px-4 py-2">
          <summary className="flex list-none cursor-pointer items-center gap-1.5 rounded-sm text-sm text-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-primary [&::-webkit-details-marker]:hidden">
            <svg
              aria-hidden="true"
              viewBox="0 0 20 20"
              fill="currentColor"
              className="h-3.5 w-3.5 shrink-0 text-warn"
            >
              <path
                fillRule="evenodd"
                clipRule="evenodd"
                d="M8.485 2.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.17 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 2.495zM10 5a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 0110 5zm0 9a1 1 0 100-2 1 1 0 000 2z"
              />
            </svg>
            <span>{failures.length} 项导出失败</span>
            <span className="text-xs text-muted-foreground">（点击查看详情）</span>
          </summary>
          <ul className="mt-2 max-h-40 space-y-1 overflow-y-auto pl-5 text-xs">
            {failures.map((f, i) => (
              <li key={`${f.source}-${i}`} className="break-all" title={f.source}>
                <span className="text-foreground">{basename(f.source)}</span>
                <span className="text-muted-foreground"> — {f.reason}</span>
              </li>
            ))}
          </ul>
        </details>
      )}
      <div className="min-h-0 flex-1">
        <GroupBrowseView onCompare={onCompare} />
      </div>
    </div>
  );
}
