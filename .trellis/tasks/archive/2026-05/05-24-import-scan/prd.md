# 导入与扫描管线

> Parent: `05-24-milestone-1-mvp`｜覆盖 ROADMAP M1 功能 ①格式 ②文件夹导入

## Goal

实现「选文件夹 → 递归扫描 → 过滤受支持格式 → 生成稳定 ID → 增量写入 `photos` 表」的导入管线，作为整个 M1 数据流的入口。产出 `status='pending'` 的照片行供后续分析消费。

## Scope

### In Scope

1. 前端：`open()` 选目录 → `invoke('scan_folder', { path })` → 拿回基础元数据列表渲染。
2. Rust：`walkdir` 递归遍历，按扩展名过滤 `jpg/jpeg/png/heic/heif`（仅看扩展名，不解码）。
3. 每个文件 `blake3` 生成 `id`（ARCHITECTURE 约定的稳定 ID）。
4. 增量索引：已存在（按 `path` UNIQUE）的不重复哈希、不重复插入。
5. 写入 `photos`（`id/path/status='pending'/created_at`），返回基础列表给前端。

### Out of Scope

- 任何分析（EXIF/模糊/曝光/pHash）→ analysis-subsystem。
- 缩略图生成（M2）。
- 导入进度可视化事件（M2）；本任务可同步返回或一次性返回列表。

## 决策点

- HEIC：扫描层只看扩展名（ROADMAP 功能①已定），不在此任务解码。
- 增量去重以 `path` UNIQUE 为准；同图不同路径视为两条（M1 不做内容级去重）。

## 软依赖

无。本任务是数据流源头，产出 `photos` 行。可独立验证。

## Acceptance Criteria

- [ ] 选一个含子目录、混合 JPG/PNG/HEIC + 无关文件（txt/mp4）的文件夹，导入后 `photos` 仅含受支持图片行，HEIC 也在内。
- [ ] 重复导入同一文件夹不新增行、不重复哈希（行数不变）。
- [ ] 每行 `id` 为 blake3 hex、`status='pending'`、`created_at` 为 ISO8601。
- [ ] 大目录（数百张）扫描在 `spawn_blocking` / rayon 中执行，不卡 UI。
- [ ] 三层 formatter 通过（prettier / rustfmt / ruff）。
