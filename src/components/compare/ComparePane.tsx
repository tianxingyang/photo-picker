import { useEffect, useRef, useState } from "react";

import { HeicPlaceholder } from "../browse/HeicPlaceholder";
import { useGroupsStore } from "../../store/groupsStore";
import { useDisplaySrc } from "../../hooks/useDisplaySrc";
import type { PhotoId } from "../../types/photo";

type View = { scale: number; tx: number; ty: number };

type ComparePaneProps = {
  photoId: PhotoId | null;
  view: View;
  side: "left" | "right";
  /** Pointer-event handlers forwarded from ABCompareViewer for zoom/pan. */
  onWheel: (e: React.WheelEvent<HTMLDivElement>) => void;
  onPointerDown: (e: React.PointerEvent<HTMLDivElement>) => void;
  onPointerMove: (e: React.PointerEvent<HTMLDivElement>) => void;
  onPointerUp: (e: React.PointerEvent<HTMLDivElement>) => void;
};

/** Loading skeleton — shown while HEIC transcode is in progress. */
function CompareLoading({ name }: { name: string }) {
  return (
    <div className="flex h-full w-full flex-col items-center justify-center gap-2 bg-background">
      {/* Shimmer bar */}
      <div className="h-4 w-40 animate-pulse rounded bg-surface" aria-hidden="true" />
      <p className="text-sm text-muted-foreground">解码中…</p>
      <p className="max-w-[80%] truncate text-xs text-muted-foreground" title={name}>
        {name}
      </p>
    </div>
  );
}

export type { View };

export function ComparePane({
  photoId,
  view,
  side,
  onWheel,
  onPointerDown,
  onPointerMove,
  onPointerUp,
}: ComparePaneProps) {
  const photo = useGroupsStore((s) => (photoId !== null ? s.byId[photoId] : undefined));
  const displaySrc = useDisplaySrc(
    // Pass a stable "null" photo when photoId is null so hooks always run.
    photo ?? {
      id: "" as PhotoId,
      name: "",
      src: "" as never,
      isHeic: false,
      status: "pending",
      shotAt: null,
      blurScore: null,
      isBlurry: null,
      exposureFlag: null,
      analysisState: "pending",
      thumbStatus: "pending",
      thumbSrc: null,
    },
  );

  // Delay showing the loading skeleton for 300 ms to avoid a flash on fast
  // transcodes (progressive-loading pattern).
  const [showLoading, setShowLoading] = useState(false);
  const delayRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (displaySrc.state === "loading") {
      delayRef.current = setTimeout(() => setShowLoading(true), 300);
    } else {
      if (delayRef.current !== null) clearTimeout(delayRef.current);
      setShowLoading(false);
    }
    return () => {
      if (delayRef.current !== null) clearTimeout(delayRef.current);
    };
  }, [displaySrc.state]);

  const label = side === "left" ? "左" : "右";

  return (
    <div
      className="relative flex flex-1 cursor-grab select-none items-center justify-center overflow-hidden bg-background active:cursor-grabbing"
      onWheel={onWheel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      role="img"
      aria-label={photo ? `${label}侧照片：${photo.name}` : `${label}侧（无照片）`}
    >
      {photoId === null || photo === undefined ? (
        // Empty slot — no group peers to compare.
        <div className="flex flex-col items-center gap-2 text-muted-foreground">
          <p className="text-sm">无同组照片可对比</p>
        </div>
      ) : displaySrc.state === "error" ? (
        <div className="flex flex-col items-center gap-2">
          <HeicPlaceholder name={photo.name} />
          <p className="text-xs text-muted-foreground">无法解码此 HEIC</p>
        </div>
      ) : displaySrc.state === "loading" && showLoading ? (
        <CompareLoading name={photo.name} />
      ) : displaySrc.state === "ready" ? (
        <img
          src={displaySrc.src}
          alt={photo.name}
          draggable={false}
          className="max-h-full max-w-full object-contain"
          style={{
            transform: `translate(${view.tx}px, ${view.ty}px) scale(${view.scale})`,
            transformOrigin: "center",
            // GPU-composited transform — no reflow.
            willChange: "transform",
          }}
        />
      ) : null}
    </div>
  );
}
