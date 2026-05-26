import { cn } from "../../lib/utils";
import type { ExposureFlag } from "../../types/photo";

import { BlurIcon, OverIcon, UnderIcon } from "./icons";

type QualityBadgesProps = {
  isBlurry: boolean | null;
  exposureFlag: ExposureFlag | null;
  className?: string;
};

type Badge = {
  key: string;
  label: string;
  cls: string;
  icon: (p: { className?: string }) => React.ReactNode;
};

const BADGE_BASE =
  "inline-flex items-center gap-1 rounded bg-black/55 px-1.5 py-0.5 text-[11px] font-medium backdrop-blur";

export function QualityBadges({ isBlurry, exposureFlag, className }: QualityBadgesProps) {
  const badges: Badge[] = [];
  if (isBlurry === true) {
    badges.push({ key: "blur", label: "模糊", cls: "text-warn", icon: BlurIcon });
  }
  if (exposureFlag === "over") {
    badges.push({ key: "over", label: "过曝", cls: "text-warn", icon: OverIcon });
  } else if (exposureFlag === "under") {
    badges.push({ key: "under", label: "欠曝", cls: "text-info", icon: UnderIcon });
  }
  if (badges.length === 0) return null;

  return (
    <div className={cn("flex flex-col items-end gap-1", className)}>
      {badges.map(({ key, label, cls, icon: Icon }) => (
        <span key={key} className={cn(BADGE_BASE, cls)}>
          <Icon className="h-3 w-3" />
          {label}
        </span>
      ))}
    </div>
  );
}
