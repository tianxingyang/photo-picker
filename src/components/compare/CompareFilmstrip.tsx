import { useRef } from "react";

import { ThumbImage } from "../browse/ThumbImage";
import { ArrowLeftRightIcon } from "../browse/icons";
import { useGroupsStore } from "../../store/groupsStore";
import { useCompareStore } from "../../store/compareStore";
import { cn } from "../../lib/utils";
import type { PhotoId } from "../../types/photo";

type ThumbnailProps = {
  id: PhotoId;
  isA: boolean;
  isB: boolean;
  onClick: () => void;
};

function Thumbnail({ id, isA, isB, onClick }: ThumbnailProps) {
  const photo = useGroupsStore((s) => s.byId[id]);
  if (!photo) return null;

  const badge = isA ? "A" : isB ? "B" : null;

  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "relative aspect-square h-16 w-16 shrink-0 overflow-hidden rounded-md border-2 bg-surface",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-primary",
        "transition-all duration-150",
        isA || isB ? "border-primary" : "border-transparent hover:border-border",
      )}
      aria-label={`选择 ${photo.name} 为 ${isB ? "B" : "对比图"}`}
      aria-pressed={isA || isB}
    >
      <ThumbImage photo={photo} />

      {/* Corner badge: A or B — uses text + position, not color alone */}
      {badge !== null && (
        <span
          className={cn(
            "absolute left-0.5 top-0.5 flex h-4 w-4 items-center justify-center",
            "rounded-sm text-[10px] font-bold leading-none",
            "bg-primary text-white",
          )}
          aria-hidden="true"
        >
          {badge}
        </span>
      )}
    </button>
  );
}

export function CompareFilmstrip() {
  const memberIds = useCompareStore((s) => s.memberIds);
  const aId = useCompareStore((s) => s.aId);
  const bId = useCompareStore((s) => s.bId);
  const setB = useCompareStore((s) => s.setB);
  const swap = useCompareStore((s) => s.swap);

  const scrollRef = useRef<HTMLDivElement | null>(null);

  if (memberIds.length <= 1) {
    // Single photo / ungrouped — filmstrip is not useful.
    return null;
  }

  return (
    <div className="flex items-center gap-2 border-t border-border bg-background px-3 py-2">
      {/* Scrollable thumbnails */}
      <div
        ref={scrollRef}
        className="flex flex-1 gap-2 overflow-x-auto"
        style={{ scrollbarWidth: "none" }}
        role="listbox"
        aria-label="组内照片"
      >
        {memberIds.map((id) => (
          <Thumbnail
            key={id}
            id={id}
            isA={id === aId}
            isB={id === bId}
            onClick={() => {
              // Clicking A is a no-op (A is the reference, not swapped here).
              if (id !== aId) setB(id);
            }}
          />
        ))}
      </div>

      {/* Swap A↔B button */}
      <button
        type="button"
        onClick={swap}
        disabled={aId === null || bId === null}
        className={cn(
          "flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border",
          "text-muted-foreground transition-colors hover:bg-surface hover:text-foreground",
          "focus:outline-none focus-visible:ring-2 focus-visible:ring-primary",
          "disabled:cursor-not-allowed disabled:opacity-40",
        )}
        aria-label="左右互换 A 和 B"
      >
        <ArrowLeftRightIcon className="h-4 w-4" />
      </button>
    </div>
  );
}
