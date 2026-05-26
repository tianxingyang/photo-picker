// Inline SVG icons (stroke = currentColor). Project rule: SVG, never emoji.
// Kept minimal — only what the browse cards need.

type IconProps = { className?: string };

function base(className: string | undefined, children: React.ReactNode) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export function CheckIcon({ className }: IconProps) {
  return base(className, <path d="M20 6 9 17l-5-5" />);
}

export function XIcon({ className }: IconProps) {
  return base(className, <path d="M18 6 6 18M6 6l12 12" />);
}

export function DotIcon({ className }: IconProps) {
  return base(className, <circle cx="12" cy="12" r="8" />);
}

export function BlurIcon({ className }: IconProps) {
  return base(
    className,
    <path d="M12 22a7 7 0 0 0 7-7c0-3-3-6-7-11C8 9 5 12 5 15a7 7 0 0 0 7 7z" />,
  );
}

export function OverIcon({ className }: IconProps) {
  return base(
    className,
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </>,
  );
}

export function UnderIcon({ className }: IconProps) {
  return base(className, <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z" />);
}

export function ImageIcon({ className }: IconProps) {
  return base(
    className,
    <>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <circle cx="9" cy="9" r="2" />
      <path d="m21 15-3.6-3.6a2 2 0 0 0-2.8 0L6 20" />
    </>,
  );
}
