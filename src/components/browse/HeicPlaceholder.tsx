import { ImageIcon } from "./icons";

type HeicPlaceholderProps = {
  name: string;
};

// HEIC can't render in the webview <img>; show a same-size placeholder so the
// photo is still reviewable. Real decode path is deferred to ab-compare (D5).
export function HeicPlaceholder({ name }: HeicPlaceholderProps) {
  return (
    <div className="flex h-full w-full flex-col items-center justify-center gap-1 bg-surface text-muted-foreground">
      <ImageIcon className="h-8 w-8" />
      <span className="text-[11px] font-medium">.HEIC</span>
      <span className="max-w-[90%] truncate px-2 text-[10px]" title={name}>
        {name}
      </span>
    </div>
  );
}
