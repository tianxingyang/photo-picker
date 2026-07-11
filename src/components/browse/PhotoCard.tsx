import { memo } from "react";

import { cn } from "../../lib/utils";
import { useGroupsStore } from "../../store/groupsStore";
import type { PhotoId, PhotoStatus } from "../../types/photo";

import { CardActions } from "./CardActions";
import { QualityBadges } from "./QualityBadges";
import { StatusPill } from "./StatusPill";
import { ThumbImage } from "./ThumbImage";

type PhotoCardProps = {
  id: PhotoId;
  onCompare: (id: PhotoId) => void;
};

function PhotoCardImpl({ id, onCompare }: PhotoCardProps) {
  // Per-id selector — never read the whole store inside a list item.
  const photo = useGroupsStore((s) => s.byId[id]);
  const setStatus = useGroupsStore((s) => s.setStatus);
  if (!photo) return null;

  const handleStatus = (status: PhotoStatus) => {
    setStatus(id, status).catch((e) => {
      // Silent rollback: the status pill flipping back is the feedback. A local
      // single-row UPDATE almost never fails, so no toast/notice is warranted.
      if (import.meta.env.DEV) console.debug("setStatus failed", e);
    });
  };

  return (
    <div className="group relative aspect-square overflow-hidden rounded-lg border border-border bg-surface">
      {/* Cover button = enter A/B compare. Real viewer lands in ab-compare. */}
      <button
        type="button"
        onClick={() => onCompare(id)}
        aria-label={`对比 ${photo.name}`}
        className="absolute inset-0 block focus:outline-none focus-visible:ring-2 focus-visible:ring-primary"
      >
        <ThumbImage photo={photo} />
      </button>

      <StatusPill status={photo.status} className="pointer-events-none absolute left-1.5 top-1.5" />
      <QualityBadges
        isBlurry={photo.isBlurry}
        exposureFlag={photo.exposureFlag}
        className="pointer-events-none absolute right-1.5 top-1.5"
      />
      <CardActions
        current={photo.status}
        onSetStatus={handleStatus}
        className={cn(
          "absolute inset-x-0 bottom-0 opacity-0 transition-opacity duration-200",
          "focus-within:opacity-100 group-hover:opacity-100",
        )}
      />
    </div>
  );
}

export const PhotoCard = memo(PhotoCardImpl);
