# Implement — 缩略图后台批量预生成 + WebP 缓存

> 执行计划。设计依据见 `design.md`，验收见 `prd.md`。按序执行；每个 checkpoint 后跑对应校验。
> 约束：在 PR 分支开发（`main` 受保护，不直接提交/推送）；工具交互英文、对用户中文。

## Pre-flight

- [ ] P0 创建并切到任务分支（如 `feat/m2-thumbnail-pregen`），`task.py set-branch` 记录。
- [ ] P1 复核 `scanner` upsert 仍为 `INSERT OR IGNORE`（`scanner/mod.rs:81,246`）——若已变更，回 design §4 重核自愈方案。

## Step 1 — DB migration（数据地基，先行）

- [ ] 1.1 新建 `src-tauri/migrations/0005_thumbnails.sql`：3 个 `ADD COLUMN`（`thumb_status` + CHECK、`thumb_src_mtime`、`thumb_error`）+ `CREATE INDEX idx_photos_thumb_status`（见 design §2.1）。
- [ ] 1.2 在 `src-tauri/src/db/mod.rs` 的 `MIGRATIONS` 数组追加 `include_str!("../../migrations/0005_thumbnails.sql")`。
- [ ] 1.3 更新/新增迁移测试：断言 `user_version == 5` 且 `== MIGRATIONS.len()`；新列存在且默认 `pending`。
- **校验**：`cargo test -p photo-picker db::` （若命中 stale fingerprint 报 0.5s no-recompile，先 `cargo clean -p photo-picker`）。
- **Review gate ①**：schema 形状与 0002/0004 一致、无 DROP/重建。
- **Rollback point**：本步独立；删文件 + 回退数组即还原。

## Step 2 — Python sidecar `thumbnail` op

- [ ] 2.1 新建 `python/analyzers/thumbnail.py`：克隆 `transcode.py` 结构——`register_heif_opener()`、`Image.open` → `ImageOps.exif_transpose` → `convert('RGB')` → 最长边 resize 到 `maxSide`(默认 512) → `img.save(tmp, format='WEBP', quality)` → 原子 `.part`→`os.replace`、`os.makedirs`。返回 `{dest,width,height}`，按文件错误 `raise`。
- [ ] 2.2 在 `python/main.py` 的 `OPS` 字典注册 `"thumbnail"`。
- **校验**：手动跑一次 sidecar REPL，喂 `{"id":1,"op":"thumbnail","payload":{"path":"<样张>","dest":"<tmp>.webp","maxSide":512,"quality":80}}`，确认产出合法 WebP 且 EXIF 旋转过的样张朝向正确；HEIC 样张也能出图。用 `uv` 管理 Python 环境运行。
- **Rollback point**：新增文件 + 一行注册，删除即还原。

## Step 3 — Rust storage helper

- [ ] 3.1 在 `src-tauri/src/db/mod.rs`（紧挨 `db_path`）加 `thumbnails_dir(app) -> app_data_dir()/thumbnails`。
- [ ] 3.2 加单照片路径推导 `thumb_dest(app, id) -> thumbnails_dir/<id[0:2]>/<id>.webp`（供 command 与 DTO 投影共用）。
- **校验**：`cargo check -p photo-picker`。

## Step 4 — Rust command `generate_thumbnails` / `cancel_thumbnails`

- [ ] 4.1 `AppState` 加 `thumbnails_running` / `thumbnails_cancel` AtomicBool（`lib.rs`），**独立**于 analysis 标志。
- [ ] 4.2 `commands/` 新增 `generate_thumbnails`：单飞 guard + RunGuard、`current_project` 作用域、stat 过滤 pass（design §3.3 判定表）、`buffer_unordered(n)` over `sidecar_pool()`、per-photo UPDATE done/failed、共享 `AtomicUsize` 进度、终态 tick。
- [ ] 4.3 `cancel_thumbnails` 置 `thumbnails_cancel`（镜像 `cancel_analysis`）。
- [ ] 4.4 在 `lib.rs` `generate_handler!` 注册两命令。
- **校验**：`cargo clippy -p photo-picker --all-targets` 无 warning；新增单元/集成测试覆盖 stat 过滤判定表（pending/done-stale/done-fresh/failed-same-mtime）与取消保持 pending。
- **Review gate ②**：确认未复用 analysis 标志（design §7）；取消语义与 analyze 一致。
- **Rollback point**：命令未接前端前，后端改动自成一体。

## Step 5 — Progress 接线

- [ ] 5.1 `commands/progress.rs` 加 `PHASE_THUMBNAIL = "thumbnail"`。
- [ ] 5.2 前端 `src/store/progressStore.ts`：`ProgressPhase` 类型 + `PHASES` 数组加 `"thumbnail"`。
- [ ] 5.3 `src/components/pipeline/PipelineProgressBar.tsx`：加中文标签分支（"生成缩略图" done/total）。
- **校验**：`npm run lint && npx tsc --noEmit`（或项目既有脚本）。

## Step 6 — DTO 投影到前端

- [ ] 6.1 `grouping.rs` 的 `load_browse_model` SELECT 与 `read_photo` 行映射加 `thumb_status`；`thumb_status='done'` 时用 `thumb_dest` 计算并返回绝对 `thumb_path`。
- [ ] 6.2 `src/types/photo.ts` 的 `BrowsePhoto` 加 `thumbSrc?: PhotoSrc | null`。
- [ ] 6.3 `src/api/groupsApi.ts` `toBrowsePhoto()`：`thumbSrc = raw.thumbPath ? convertFileSrc(raw.thumbPath) : null`。
- **校验**：`cargo check` + `tsc --noEmit`。

## Step 7 — 前端渲染改用缩略图

- [ ] 7.1 `PhotoCard.tsx`：`src={photo.thumbSrc ?? photo.src}` + `onError` 回退 `photo.src`。
- [ ] 7.2 `CompareFilmstrip.tsx` `Thumbnail`：同样改用 `thumbSrc`（+ onError 回退）。
- [ ] 7.3 HEIC 分支改为 `thumbSrc ? <img thumb> : isHeic ? <HeicPlaceholder> : <img original>`（design §6）。
- [ ] 7.4 `ComparePane.tsx` 保持不动（大图全分辨率）。
- **校验**：`tsc --noEmit` + lint。涉及可见渲染，若新增任何可见控件须走 `ui-ux-pro-max`；本步仅 `<img src>` 切换 + onError，无新控件。

## Step 8 — 触发编排（前端链路）

- [ ] 8.1 新建 `src/api/thumbnailsApi.ts`：`generateThumbnails()` / `cancelThumbnails()`。
- [ ] 8.2 `App.tsx`：`scanFolder` 后、`analyzePending` 前 `await generateThumbnails()`（顺序执行确保与分析池互斥）。
- [ ] 8.3 取消 UI 接入 `cancelThumbnails`（沿用现有 pipeline 取消入口）。

## Step 9 — 删除项目清理（AC11）

- [ ] 9.1 `projects.rs` `delete_project`：级联删除前 `SELECT id FROM photos WHERE project_id=?`，对每 id `remove_file(thumb_dest)`（best-effort，忽略 NotFound），再走原删除。
- **校验**：`cargo test` 覆盖删除后缩略图文件消失。

## Step 10 — 集成验证

- [ ] 10.1 `cargo test -p photo-picker`（全绿）、`cargo clippy`（无 warning）、`npm run lint`、`tsc --noEmit`。
- [ ] 10.2 跑 `trellis-check` 做 spec 合规 + 跨层数据流复核。
- [ ] 10.3 **GUI 冒烟（由用户驱动，我只给步骤）**——dev 端口 5173（1420 保留）：
  1. 启动 dev app，新建/打开项目，导入含 JPG/PNG/HEIC 的文件夹。
  2. 观察顶部进度条出现"生成缩略图 done/total"，随后才进入分析。
  3. 网格与对比 filmstrip 出图；devtools 确认 asset URL 指向 `…/thumbnails/…webp`（AC4）；HEIC 瓦片显示真实预览而非占位（AC5）。
  4. 检查 `<app_data_dir>/thumbnails/<id[0:2]>/<id>.webp` 已生成、最长边 ≤512（AC3）。
  5. 重新导入同文件夹 → 缩略图阶段近乎瞬时跳过（AC6）；导入中途点取消 → 剩余保持 pending、再导入续完。
  6. 手动改一张源图 mtime（或替换文件）后重跑 → 该缩略图被重生成（AC7）。
  7. 删除项目 → 对应缩略图文件被清理（AC11）。
- [ ] 10.4 逐条对照 `prd.md` AC1–AC11 勾选。

## Finish（Phase 3）

- [ ] F1 `trellis-update-spec`：把"缩略图缓存契约/mtime 自愈/池互斥"沉淀进 `.trellis/spec/backend`；同步更新 `ARCHITECTURE.md:163`（mtime 自愈说明）与 `ROADMAP.md`（M2 该项打勾 + 决策池"分辨率档位"标记单档 512px 已定）。
- [ ] F2 提交到 PR 分支并开 PR（不碰 main）。
- [ ] F3 `task.py finish` + 归档。

## 校验命令速查

| 目的 | 命令 |
|---|---|
| Rust 测试 | `cargo test -p photo-picker`（stale 时先 `cargo clean -p photo-picker`） |
| Rust lint | `cargo clippy -p photo-picker --all-targets` |
| 前端类型 | `npx tsc --noEmit` |
| 前端 lint | `npm run lint` |
| Python 环境 | 一律 `uv` 管理 |
| Dev app | 端口 5173（注意 `vite.config.js` 遮蔽 `.ts`） |
