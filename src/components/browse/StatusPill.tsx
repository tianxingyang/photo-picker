import { cn } from "../../lib/utils";
import type { PhotoStatus } from "../../types/photo";

import { CheckIcon, DotIcon, XIcon } from "./icons";

type StatusPillProps = {
  status: PhotoStatus;
  className?: string;
};

// Three-channel signal (icon + text + colour), never colour alone — a11y rule
// color-not-only. Dark scrim keeps it legible over any photo.
const PILL: Record<PhotoStatus, { label: string; cls: string }> = {
  keep: { label: "保留", cls: "text-keep border-keep/60" },
  reject: { label: "淘汰", cls: "text-reject border-reject/60" },
  pending: { label: "待定", cls: "text-pending border-pending/60" },
};

export function StatusPill({ status, className }: StatusPillProps) {
  const { label, cls } = PILL[status];
  const Icon = status === "keep" ? CheckIcon : status === "reject" ? XIcon : DotIcon;
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-full border bg-black/55 px-1.5 py-0.5 text-[11px] font-medium backdrop-blur",
        cls,
        className,
      )}
    >
      <Icon className="h-3 w-3" />
      {label}
    </span>
  );
}
