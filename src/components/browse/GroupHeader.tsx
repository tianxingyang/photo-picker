type GroupHeaderProps = {
  title: string;
  count: number;
};

export function GroupHeader({ title, count }: GroupHeaderProps) {
  return (
    <div className="flex items-baseline gap-2 px-4 pb-2 pt-4">
      <h2 className="text-sm font-semibold text-foreground">{title}</h2>
      <span className="text-xs tabular-nums text-muted-foreground">{count} 张</span>
    </div>
  );
}
