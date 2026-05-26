import { cn } from "../../lib/utils";
import type { PhotoStatus } from "../../types/photo";

import { CheckIcon, DotIcon, XIcon } from "./icons";

type CardActionsProps = {
  current: PhotoStatus;
  onSetStatus: (status: PhotoStatus) => void;
  className?: string;
};

const ACTIONS: { status: PhotoStatus; label: string; activeCls: string }[] = [
  { status: "keep", label: "保留", activeCls: "bg-keep text-white" },
  { status: "reject", label: "淘汰", activeCls: "bg-reject text-white" },
  { status: "pending", label: "待定", activeCls: "bg-pending text-black" },
];

function ActionIcon({ status }: { status: PhotoStatus }) {
  if (status === "keep") return <CheckIcon className="h-3 w-3" />;
  if (status === "reject") return <XIcon className="h-3 w-3" />;
  return <DotIcon className="h-3 w-3" />;
}

// Mount point for status toggling. Optimistic update happens in the store;
// the keep-reject-status task wires the actual persistence. Always keyboard
// reachable (real buttons), even while visually hidden until hover/focus.
export function CardActions({ current, onSetStatus, className }: CardActionsProps) {
  return (
    <div className={cn("flex gap-px bg-black/60 p-1 backdrop-blur", className)}>
      {ACTIONS.map(({ status, label, activeCls }) => {
        const active = current === status;
        return (
          <button
            key={status}
            type="button"
            aria-pressed={active}
            onClick={() => onSetStatus(status)}
            className={cn(
              "flex flex-1 items-center justify-center gap-1 rounded px-1 py-1 text-[11px] text-foreground/80",
              "hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary",
              active && activeCls,
            )}
          >
            <ActionIcon status={status} />
            {label}
          </button>
        );
      })}
    </div>
  );
}
