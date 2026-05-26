import { useEffect, useMemo, useRef, useState } from "react";

import { useVirtualizer } from "@tanstack/react-virtual";

import { useGroupsStore } from "../../store/groupsStore";
import type { BrowseGroup } from "../../types/group";
import type { PhotoId } from "../../types/photo";

import { EmptyState } from "./EmptyState";
import { GroupHeader } from "./GroupHeader";
import { PhotoRow } from "./PhotoRow";

type GroupBrowseViewProps = {
  onCompare: (id: PhotoId) => void;
};

type Row =
  | { kind: "header"; key: string; title: string; count: number }
  | { kind: "photos"; key: string; ids: PhotoId[] };

const TILE_MIN = 180; // px target tile width -> column count
const GAP = 12;
const HEADER_ESTIMATE = 44;
const ROW_ESTIMATE = 200;

function chunk(ids: PhotoId[], cols: number): PhotoId[][] {
  const out: PhotoId[][] = [];
  for (let i = 0; i < ids.length; i += cols) out.push(ids.slice(i, i + cols));
  return out;
}

function buildRows(groups: BrowseGroup[], ungroupedIds: PhotoId[], cols: number): Row[] {
  const rows: Row[] = [];
  groups.forEach((g, i) => {
    rows.push({
      kind: "header",
      key: `h-${g.id}`,
      title: `相似组 ${i + 1}`,
      count: g.photoIds.length,
    });
    chunk(g.photoIds, cols).forEach((ids, j) =>
      rows.push({ kind: "photos", key: `r-${g.id}-${j}`, ids }),
    );
  });
  if (ungroupedIds.length > 0) {
    rows.push({ kind: "header", key: "h-ungrouped", title: "未分组", count: ungroupedIds.length });
    chunk(ungroupedIds, cols).forEach((ids, j) =>
      rows.push({ kind: "photos", key: `r-ungrouped-${j}`, ids }),
    );
  }
  return rows;
}

// Single scroll container, flattened header+photo-row model, virtualized via
// TanStack Virtual with dynamic measurement (square tiles vary with width).
export function GroupBrowseView({ onCompare }: GroupBrowseViewProps) {
  const groups = useGroupsStore((s) => s.groups);
  const ungroupedIds = useGroupsStore((s) => s.ungroupedIds);
  const loaded = useGroupsStore((s) => s.loaded);

  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return undefined;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width;
      if (typeof w === "number") setWidth(w);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const cols = width > 0 ? Math.max(1, Math.floor((width + GAP) / (TILE_MIN + GAP))) : 4;
  const rows = useMemo(() => buildRows(groups, ungroupedIds, cols), [groups, ungroupedIds, cols]);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => (rows[i]?.kind === "header" ? HEADER_ESTIMATE : ROW_ESTIMATE),
    overscan: 6,
  });

  return (
    <div ref={scrollRef} className="h-full overflow-auto">
      {loaded && rows.length === 0 ? (
        <EmptyState />
      ) : (
        <div className="relative w-full" style={{ height: `${virtualizer.getTotalSize()}px` }}>
          {virtualizer.getVirtualItems().map((vi) => {
            const row = rows[vi.index];
            if (!row) return null;
            return (
              <div
                key={row.key}
                data-index={vi.index}
                ref={virtualizer.measureElement}
                className="absolute left-0 top-0 w-full"
                style={{ transform: `translateY(${vi.start}px)` }}
              >
                {row.kind === "header" ? (
                  <GroupHeader title={row.title} count={row.count} />
                ) : (
                  <PhotoRow ids={row.ids} cols={cols} onCompare={onCompare} />
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
