import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";

// Mirrors the Rust `ProgressEvent` (commands/progress.rs), serde camelCase.
export type ProgressPhase = "import" | "analyze" | "group";
export type ProgressStatus = "running" | "done" | "cancelled" | "error";

export type Progress = {
  phase: ProgressPhase;
  done: number;
  /** null = indeterminate (import-discovering, grouping). */
  total: number | null;
  status: ProgressStatus;
};

type ProgressState = {
  /** null = idle (bar hidden). */
  current: Progress | null;
};

export const useProgressStore = create<ProgressState>(() => ({ current: null }));

const PHASES: readonly string[] = ["import", "analyze", "group"];
const STATUSES: readonly string[] = ["running", "done", "cancelled", "error"];

function isProgress(v: unknown): v is Progress {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.phase === "string" &&
    PHASES.includes(o.phase) &&
    typeof o.done === "number" &&
    (o.total === null || typeof o.total === "number") &&
    typeof o.status === "string" &&
    STATUSES.includes(o.status)
  );
}

// why: a terminal (done/cancelled) tick should linger briefly so the bar's
// completion is visible, then clear — UNLESS the next phase starts immediately
// (analyze→group), in which case the incoming running tick cancels the clear.
let clearTimer: ReturnType<typeof setTimeout> | null = null;
let started = false;
let unlisten: (() => void) | null = null;

const TERMINAL_LINGER_MS = 600;

/** Subscribe to backend pipeline progress once (idempotent). Call from App. */
export function initProgressListener(): void {
  if (started) return;
  started = true;
  void listen("pipeline://progress", (event) => {
    if (!isProgress(event.payload)) return;
    const p = event.payload;
    if (clearTimer !== null) {
      clearTimeout(clearTimer);
      clearTimer = null;
    }
    useProgressStore.setState({ current: p });
    if (p.status !== "running") {
      clearTimer = setTimeout(() => {
        useProgressStore.setState({ current: null });
        clearTimer = null;
      }, TERMINAL_LINGER_MS);
    }
  }).then((un) => {
    unlisten = un;
  });
}

// why: in Vite dev HMR this module is re-evaluated, resetting `started` to false
// while the previous listen() subscription stays live — re-init would then
// double-subscribe. Tear the old listener (and pending timer) down on dispose so
// the fresh module re-subscribes exactly once. No-op / tree-shaken in production.
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    unlisten?.();
    if (clearTimer !== null) clearTimeout(clearTimer);
  });
}
