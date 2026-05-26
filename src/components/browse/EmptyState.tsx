export function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
      <p className="text-sm">还没有可浏览的照片</p>
      <p className="text-xs">导入文件夹并完成分析与分组后，这里会按相似组展示。</p>
    </div>
  );
}
