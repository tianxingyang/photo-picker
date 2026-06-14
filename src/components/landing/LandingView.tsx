import { useEffect, useState } from "react";

import type { ProjectSummary } from "../../api/projectsApi";
import { useProjectsStore } from "../../store/projectsStore";
import { describeAppError } from "../../types/ipc";

const BTN =
  "rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary";

// Subtle secondary button on the surface, for non-primary row actions.
const GHOST_BTN =
  "rounded-md border border-border px-3 py-1.5 text-sm font-medium text-foreground transition-colors hover:bg-border disabled:cursor-not-allowed disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary";

/** Format an ISO8601 instant as a calm local "YYYY-MM-DD HH:mm"; null → 从未打开. */
function formatLastOpened(iso: string | null): string {
  if (iso === null) return "从未打开";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(
    d.getMinutes(),
  )}`;
}

export function LandingView() {
  const projects = useProjectsStore((s) => s.projects);
  const loaded = useProjectsStore((s) => s.loaded);
  const loadProjects = useProjectsStore((s) => s.loadProjects);
  const open = useProjectsStore((s) => s.open);
  const create = useProjectsStore((s) => s.create);
  const remove = useProjectsStore((s) => s.remove);

  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The project queued for deletion — drives the confirm dialog (null = closed).
  const [pendingDelete, setPendingDelete] = useState<ProjectSummary | null>(null);

  useEffect(() => {
    loadProjects().catch((e) => setError(`加载项目失败：${describeAppError(e).message}`));
  }, [loadProjects]);

  async function onCreate() {
    const trimmed = name.trim();
    if (trimmed === "" || busy) return;
    setBusy(true);
    setError(null);
    try {
      // Create then open straight into an empty grid (AC6).
      const project = await create(trimmed);
      setName("");
      await open(project.id);
    } catch (e) {
      setError(`创建项目失败：${describeAppError(e).message}`);
    } finally {
      setBusy(false);
    }
  }

  async function onOpen(id: string) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await open(id);
    } catch (e) {
      setError(`打开项目失败：${describeAppError(e).message}`);
      setBusy(false); // on success the view unmounts; only reset on failure
    }
  }

  async function onConfirmDelete() {
    if (pendingDelete === null || busy) return;
    setBusy(true);
    setError(null);
    try {
      await remove(pendingDelete.id);
      setPendingDelete(null);
    } catch (e) {
      setError(`删除项目失败：${describeAppError(e).message}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="h-full overflow-y-auto bg-background">
      <div className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-10">
        <header className="flex flex-col gap-1">
          <h1 className="text-xl font-semibold text-foreground">Photo Picker</h1>
          <p className="text-sm text-muted-foreground">
            选择一个项目继续，或新建一个开始整理照片。
          </p>
        </header>

        {/* Create control — single primary CTA on this screen. */}
        <form
          className="flex items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void onCreate();
          }}
        >
          <div className="flex flex-1 flex-col gap-1">
            <label htmlFor="new-project-name" className="text-xs text-muted-foreground">
              新项目名称
            </label>
            <input
              id="new-project-name"
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="例如：2026 春季旅行"
              disabled={busy}
              className="rounded-md border border-border bg-surface px-3 py-1.5 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50"
            />
          </div>
          <button type="submit" disabled={busy || name.trim() === ""} className={BTN}>
            新建并打开
          </button>
        </form>

        {error !== null && (
          <div
            role="alert"
            aria-live="polite"
            className="rounded-md border border-border px-3 py-2 text-sm text-warn"
          >
            {error}
          </div>
        )}

        {/* Project list / empty state. */}
        {!loaded ? (
          <p className="text-sm text-muted-foreground">加载中…</p>
        ) : projects.length === 0 ? (
          <div className="rounded-md border border-dashed border-border px-4 py-10 text-center">
            <p className="text-sm text-foreground">还没有任何项目</p>
            <p className="mt-1 text-xs text-muted-foreground">
              在上方输入名称，新建你的第一个项目。
            </p>
          </div>
        ) : (
          <ul className="flex flex-col gap-2">
            {projects.map((p) => (
              <li
                key={p.id}
                className="flex items-center gap-3 rounded-md border border-border bg-surface px-4 py-3"
              >
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-foreground" title={p.name}>
                    {p.name}
                  </p>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    <span className="tabular-nums">{p.photoCount}</span> 张照片 ·{" "}
                    {formatLastOpened(p.lastOpenedAt)}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => void onOpen(p.id)}
                  disabled={busy}
                  className={BTN}
                >
                  打开
                </button>
                {/* Clear text button (not a faint icon) — the delete affordance
                    was hard to discover before. Danger-tinted, fills red on hover. */}
                <button
                  type="button"
                  onClick={() => setPendingDelete(p)}
                  disabled={busy}
                  aria-label={`删除项目 ${p.name}`}
                  className="shrink-0 inline-flex items-center gap-1 rounded-md border border-border px-2.5 py-1.5 text-sm font-medium text-reject transition-colors hover:bg-reject hover:text-white disabled:cursor-not-allowed disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                >
                  <svg
                    aria-hidden="true"
                    viewBox="0 0 20 20"
                    fill="currentColor"
                    className="h-3.5 w-3.5"
                  >
                    <path
                      fillRule="evenodd"
                      clipRule="evenodd"
                      d="M8.75 1.5a1.75 1.75 0 0 0-1.715 1.4L6.83 4H3.75a.75.75 0 0 0 0 1.5h.31l.69 10.36A2.25 2.25 0 0 0 7 18h6a2.25 2.25 0 0 0 2.25-2.14L15.94 5.5h.31a.75.75 0 0 0 0-1.5h-3.08l-.205-1.1A1.75 1.75 0 0 0 11.25 1.5h-2.5Zm2.95 2.5-.13-.7a.25.25 0 0 0-.245-.2H8.675a.25.25 0 0 0-.246.2L8.3 4h3.4ZM8.5 7.75a.75.75 0 0 1 1.5 0v5a.75.75 0 0 1-1.5 0v-5Zm3.5-.75a.75.75 0 0 0-.75.75v5a.75.75 0 0 0 1.5 0v-5A.75.75 0 0 0 12 7Z"
                    />
                  </svg>
                  删除
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* Confirm delete dialog — required before a destructive action. */}
      {pendingDelete !== null && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-6"
          role="dialog"
          aria-modal="true"
          aria-labelledby="delete-dialog-title"
          onClick={() => !busy && setPendingDelete(null)}
        >
          <div
            className="w-full max-w-sm rounded-md border border-border bg-surface p-5 shadow-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <h2 id="delete-dialog-title" className="text-base font-semibold text-foreground">
              删除项目
            </h2>
            <p className="mt-2 text-sm text-muted-foreground">
              确定删除「{pendingDelete.name}」？该项目下的照片记录与分组将被移除（不影响已导出到磁盘的副本）。此操作不可撤销。
            </p>
            <div className="mt-5 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setPendingDelete(null)}
                disabled={busy}
                className={GHOST_BTN}
              >
                取消
              </button>
              <button
                type="button"
                onClick={() => void onConfirmDelete()}
                disabled={busy}
                className="rounded-md bg-reject px-3 py-1.5 text-sm font-medium text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary"
              >
                {busy ? "删除中…" : "删除"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
