import type { PhotoId } from "../../types/photo";

import { PhotoCard } from "./PhotoCard";

type PhotoRowProps = {
  ids: PhotoId[];
  cols: number;
  onCompare: (id: PhotoId) => void;
};

// One virtual row = up to `cols` tiles in a responsive grid.
export function PhotoRow({ ids, cols, onCompare }: PhotoRowProps) {
  return (
    <div
      className="grid gap-3 px-4 pb-3"
      style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}
    >
      {ids.map((id) => (
        <PhotoCard key={id} id={id} onCompare={onCompare} />
      ))}
    </div>
  );
}
