import { useProgressStore, type Progress } from "../../store/progressStore";
import { cancelAnalysis, cancelThumbnails } from "../../api/pipelineApi";

/** Compact, non-blocking pipeline progress: a thin top loading bar under the
 *  header. Determinate (width = done/total) when `total` is known; an
 *  indeterminate sliding segment otherwise (import-discovering, grouping).
 *  Hidden when idle. Built per ui-ux-pro-max: role=progressbar + aria values,
 *  reduced-motion fallback, focus-visible ring, tabular figures. */
export function PipelineProgressBar() {
  const current = useProgressStore((s) => s.current);
  if (current === null) return null;

  const { phase, done, total, status } = current;
  const finished = status !== "running";
  const errored = status === "error";
  const determinate = total !== null && total > 0;
  const pct = determinate ? Math.min(100, Math.round((done / total) * 100)) : null;
  const label = phaseLabel(current);
  // Both the thumbnail and analyze batches are cooperatively cancellable.
  const canCancel = (phase === "analyze" || phase === "thumbnail") && status === "running";
  // why: a terminal tick (done/cancelled/error) must NOT keep the "still working"
  // sliding animation — even when total is unknown (group, or an empty batch).
  const fill = errored ? "bg-reject" : "bg-primary";

  return (
    <div className="flex items-center gap-3 border-b border-border bg-background px-4 py-1.5">
      <div
        className="relative h-1 flex-1 overflow-hidden rounded-full bg-border"
        role="progressbar"
        aria-label={label}
        aria-valuemin={determinate ? 0 : undefined}
        aria-valuemax={determinate ? 100 : undefined}
        aria-valuenow={determinate ? (pct as number) : undefined}
      >
        {determinate ? (
          <div
            className={`absolute inset-y-0 left-0 rounded-full ${fill} transition-[width] duration-200 ease-out`}
            style={{ width: `${pct}%` }}
          />
        ) : finished ? (
          // Terminal with unknown total: a filled static bar, not the animation.
          <div className={`absolute inset-0 rounded-full ${fill}`} />
        ) : (
          // Indeterminate running: a sliding segment; reduced-motion parks it centered.
          <div className="absolute inset-y-0 left-[30%] w-2/5 rounded-full bg-primary motion-safe:left-0 motion-safe:animate-[progressIndeterminate_1.2s_ease-in-out_infinite]" />
        )}
      </div>

      <span className="shrink-0 text-xs tabular-nums text-muted-foreground" aria-live="polite">
        {label}
      </span>

      {canCancel && (
        <button
          type="button"
          onClick={() => void (phase === "thumbnail" ? cancelThumbnails() : cancelAnalysis())}
          className="shrink-0 rounded-md px-2 py-0.5 text-xs font-medium text-muted-foreground transition-colors hover:text-reject focus:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          取消
        </button>
      )}
    </div>
  );
}

function phaseLabel({ phase, done, total, status }: Progress): string {
  if (status === "error") return `${phaseNoun(phase)}失败`;
  switch (phase) {
    case "import":
      return total !== null ? `导入 ${done}/${total}` : `导入 ${done}`;
    case "thumbnail":
      return total !== null ? `生成缩略图 ${done}/${total}` : `生成缩略图 ${done}`;
    case "analyze":
      return total !== null ? `分析 ${done}/${total}` : `分析 ${done}`;
    case "group":
      // group has no per-item count — reflect the terminal state, not a fixed
      // "分组中…" that would otherwise linger after grouping finished. (status
      // "error" is already handled above.)
      if (status === "done") return "分组完成";
      if (status === "cancelled") return "分组已取消";
      return "分组中…";
  }
}

function phaseNoun(phase: Progress["phase"]): string {
  switch (phase) {
    case "import":
      return "导入";
    case "thumbnail":
      return "生成缩略图";
    case "analyze":
      return "分析";
    case "group":
      return "分组";
  }
}
