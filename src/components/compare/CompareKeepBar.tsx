import { CheckIcon } from "../browse/icons";
import { useGroupsStore } from "../../store/groupsStore";
import { cn } from "../../lib/utils";
import type { PhotoId, PhotoStatus } from "../../types/photo";

type CompareKeepBarProps = {
  aId: PhotoId | null;
  bId: PhotoId | null;
};

type KeepButtonProps = {
  photoId: PhotoId | null;
  label: string;
  shortcut: string;
};

function KeepButton({ photoId, label, shortcut }: KeepButtonProps) {
  const photo = useGroupsStore((s) => (photoId !== null ? s.byId[photoId] : undefined));
  const setStatus = useGroupsStore((s) => s.setStatus);

  const isKeep: boolean = photo?.status === ("keep" as PhotoStatus);

  function handleKeep() {
    if (photoId === null) return;
    setStatus(photoId, "keep").catch((e) => {
      if (import.meta.env.DEV) console.debug("setStatus failed", e);
    });
  }

  return (
    <button
      type="button"
      onClick={handleKeep}
      disabled={photoId === null}
      className={cn(
        "flex flex-1 items-center justify-center gap-2 rounded-lg border px-4 py-3",
        "text-sm font-medium transition-all duration-150",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-primary",
        "active:scale-[.97]",
        isKeep
          ? "border-keep bg-keep/20 text-keep"
          : "border-border bg-surface text-muted-foreground hover:border-keep hover:text-keep",
        photoId === null && "cursor-not-allowed opacity-40",
      )}
      aria-label={`${label} (${shortcut})`}
      aria-pressed={isKeep}
    >
      <CheckIcon className="h-4 w-4 shrink-0" />
      <span>
        {isKeep ? "已保留" : "保留"} {label}
        <span className="ml-1 text-xs opacity-60">({shortcut})</span>
      </span>
    </button>
  );
}

export function CompareKeepBar({ aId, bId }: CompareKeepBarProps) {
  return (
    <div className="flex gap-3 border-t border-border bg-background px-4 py-3">
      <KeepButton photoId={aId} label="左" shortcut="1" />
      <KeepButton photoId={bId} label="右" shortcut="2" />
    </div>
  );
}
