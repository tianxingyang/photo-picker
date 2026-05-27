import { XIcon, ZoomInIcon, ZoomOutIcon, Maximize2Icon } from "../browse/icons";
import { cn } from "../../lib/utils";

type CompareToolbarProps = {
  /** Display label e.g. "1 / 4 · IMG_0423.HEIC" */
  label: string;
  scale: number;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onZoomReset: () => void;
  onClose: () => void;
};

const ICON_BTN = cn(
  "flex h-11 w-11 items-center justify-center rounded-md text-muted-foreground",
  "transition-colors hover:bg-surface hover:text-foreground",
  "focus:outline-none focus-visible:ring-2 focus-visible:ring-primary",
);

export function CompareToolbar({
  label,
  scale,
  onZoomIn,
  onZoomOut,
  onZoomReset,
  onClose,
}: CompareToolbarProps) {
  const pct = Math.round(scale * 100);

  return (
    <div className="flex items-center justify-between bg-surface/80 px-2 py-1 backdrop-blur-sm">
      {/* Left: close button */}
      <button type="button" onClick={onClose} className={ICON_BTN} aria-label="关闭对比 (Esc)">
        <XIcon className="h-5 w-5" />
      </button>

      {/* Center: position label + filename */}
      <span className="max-w-[50%] truncate text-sm text-muted-foreground" title={label}>
        {label}
      </span>

      {/* Right: zoom controls */}
      <div className="flex items-center gap-1">
        <button type="button" onClick={onZoomOut} className={ICON_BTN} aria-label="缩小">
          <ZoomOutIcon className="h-5 w-5" />
        </button>

        <button
          type="button"
          onClick={onZoomReset}
          className={cn(
            "rounded px-2 py-1 text-sm tabular-nums text-muted-foreground",
            "transition-colors hover:bg-surface hover:text-foreground",
            "focus:outline-none focus-visible:ring-2 focus-visible:ring-primary min-w-[3.5rem] text-center",
          )}
          aria-label="重置缩放"
        >
          {pct}%
        </button>

        <button type="button" onClick={onZoomIn} className={ICON_BTN} aria-label="放大">
          <ZoomInIcon className="h-5 w-5" />
        </button>

        <button
          type="button"
          onClick={onZoomReset}
          className={ICON_BTN}
          aria-label="适合窗口"
          title="适合窗口"
        >
          <Maximize2Icon className="h-5 w-5" />
        </button>
      </div>
    </div>
  );
}
