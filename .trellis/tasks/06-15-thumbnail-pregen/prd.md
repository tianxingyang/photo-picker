# 缩略图后台批量预生成 + WebP 缓存

## Goal

M2 体验增强：扫描导入后**自动链式**批量预生成缩略图，让网格/对比视图不再解码全分辨率原片。缩略图为单档 **512px 最长边 WebP**，落 `<app_data_dir>/thumbnails/<id[0:2]>/<id>.webp` 两级目录缓存，**mtime 感知自愈**。网格瓦片与 A/B 对比 filmstrip 改用缩略图，HEIC 首次可在网格/filmstrip 显示真实预览（此前只有文字占位）。

## Background（为什么做）

当前前端三处——网格瓦片（`PhotoCard`）、A/B 对比 filmstrip（`CompareFilmstrip` 64×64）、对比大图（`ComparePane`）——的 `<img src>` 全部直指**全分辨率原片**，靠 CSS 缩小。连 64px 的 filmstrip 都在解码数 MP 的原图，是最浪费的一处。前端无任何缩略图概念、无 srcset/DPR 处理。HEIC 因 webview 无法解码，网格/filmstrip 只显示文字占位。

## Scope

### In scope
- 单档 512px WebP 缩略图的批量预生成（含 HEIC）。
- 持久磁盘缓存 + mtime 自愈 + 幂等/可续跑 + 协作取消。
- 扫描后自动链式触发（scan → thumbnails → analyze），与分析在 sidecar 池上互斥/排队。
- DB 加 `thumb_status` 列（迁移 0005）。
- 进度条复用既有 `pipeline://progress`，新增 `thumbnail` phase。
- 前端：网格 + 对比 filmstrip 改用缩略图并优雅降级；HEIC 网格/filmstrip 显示真实预览。

### Out of scope
- 双档/多分辨率与 srcset/DPR 响应式选图（实测单档 512px 已压住 retina 最坏值；如大库 profiling 证明过重，另起任务）。
- 对比大图（`ComparePane`）改用缩略图——大图保持全分辨率/transcode 以保证缩放质量。
- 缩略图缓存的容量上限/LRU 驱逐（本期仅做删除项目/重扫时的清理，详见 AC）。
- 缩略图分辨率用户可调。

## Decisions（已拍板，连同理由记录）

1. **分辨率档位 = 单档 512px WebP @q80**。实测网格瓦片 180–270px 方形（retina 最坏 270×DPR2=540 物理像素），512px 恰好压住最坏值、对常见瓦片与 64px filmstrip 绰绰有余；一张仅 ~30–60KB。前端零 srcset/DPR 代码，双档等于从零造响应式选图，收益不抵复杂度。（对应 ROADMAP 决策池中"缩略图分辨率档位"OPEN item，及 `.trellis/spec/backend/database-guidelines.md` 的 deferred 项。）
2. **缓存键/失效 = mtime 感知自愈**。前端拿不到 `app_data_dir`，缩略图路径无论如何由后端返回，故"mtime-less 让前端自推路径"无收益；嵌 mtime 自愈几乎零成本且更稳。需相应更新 `ARCHITECTURE.md:163` 的 `<id>.webp` 命名约定（详见 design.md，倾向"`<id>.webp` 文件名不变 + 命中时校验源 mtime 决定是否重生成"）。
3. **触发 = 扫描后自动链式**（scan → thumbnails → analyze）。网格秒出图（预生成的本意），且排在 analyze 前天然避免两批次抢同一 N-sidecar 池。
4. **缓存位置 = `app_data_dir`**（遵循 `ARCHITECTURE.md:163`，持久、不被 OS 清理）。
5. **并发 = 与分析互斥/排队**（独立 `thumbnails_running`/`thumbnails_cancel` 标志，绝不复用 analysis 标志），防 2N 超订 N 个 sidecar。
6. **Python 端独立 `thumbnail` op**（克隆 `transcode.py`），而非折进 `analyze.py` 复用解码——更干净、对齐 `transcode` 先例、不改 analyze IPC 契约。

## Requirements

### Functional
- FR1 扫描导入完成后，自动对当前项目所有 `thumb_status='pending'` 的照片生成 512px 最长边 WebP 缩略图。
- FR2 缩略图落 `<app_data_dir>/thumbnails/<id[0:2]>/<id>.webp`（id = blake3(project_id+'\n'+path)，天然项目隔离）。
- FR3 命中已存在且源文件 mtime 未变的缩略图时跳过（幂等）；取消后重跑可续。
- FR4 源文件 mtime 变化时重新生成对应缩略图（自愈）。
- FR5 协作式取消：取消后未开始的照片保持 `pending`，进行中的完成后落盘。
- FR6 进度通过既有 `pipeline://progress` 事件上报，新增 `thumbnail` phase，顶部进度条显示中文标签（如"生成缩略图 done/total"）。
- FR7 HEIC 也生成 WebP 缩略图；网格与 filmstrip 在缩略图就绪后显示真实预览，取代文字占位。
- FR8 前端网格瓦片与对比 filmstrip 使用缩略图；缩略图未就绪/缺失时优雅降级为原片（`thumbSrc ?? src` + `<img onError>` 兜底）。
- FR9 与分析批次在 sidecar 池上互斥/排队，不并发抢占。

### Non-functional / Constraints
- C1 不新增 Python 依赖（Pillow WebP 插件已随 pillow 安装）。
- C2 迁移前向只增（additive `ADD COLUMN`），遵循 `.trellis/spec/backend/database-guidelines.md`；`user_version` 与 `MIGRATIONS.len()` 保持锁步（有测试断言）。
- C3 尊重项目隔离：`thumb_status` 选取按 `project_id` 作用域；缩略图文件名已含 project 维度。
- C4 `main` 受保护：在 PR 分支开发，不直接提交/推送 main。
- C5 工具交互英文、对用户输出中文（全局约束）。
- C6 任何**新增可见 UI** 须走 `ui-ux-pro-max`；本任务主要是 `<img src>` 切换与进度条中文标签，若涉及新增可见控件再走该流程。
- C7 不改 `tauri.conf.json`：`assetProtocol` scope `["**"]` + CSP 已允许加载 `app_data_dir` 下的 WebP（与 `transcode_for_display` 同机制）。

## Acceptance Criteria

- [ ] AC1 迁移 0005 使 `user_version` 升到 5；既有项目隔离测试仍绿；`thumb_status` 默认 `pending`、CHECK 约束 `('pending','done','failed')`。
- [ ] AC2 扫描一个含 JPG/PNG/HEIC 的文件夹后，缩略图阶段**自动**运行，进度条出现"生成缩略图 done/total"，随后 analyze 阶段才开始。
- [ ] AC3 每张照片在 `<app_data_dir>/thumbnails/<id[0:2]>/<id>.webp` 生成一个合法 WebP，最长边 ≤512px。
- [ ] AC4 网格瓦片与对比 filmstrip 从 WebP 渲染（asset URL 指向 thumbnails 目录），而非原片；64px filmstrip 不再解码全分辨率原图。
- [ ] AC5 HEIC 瓦片在缩略图就绪后于网格 + filmstrip 显示真实预览（不再是文字占位）。
- [ ] AC6 重跑生成命令跳过已完成项（近乎瞬时）；中途取消后剩余项保持 `pending`，重跑可续完。
- [ ] AC7 原地修改某源文件（mtime 变化）后再次生成时，其缩略图被重新生成（自愈，不显示陈旧图）。
- [ ] AC8 取消分析不影响缩略图状态，反之亦然（独立标志，互不串扰）。
- [ ] AC9 对比大图 A/B 仍使用全分辨率/transcode 图，缩放质量不受影响。
- [ ] AC10 缩略图文件缺失（手动删除/项目删除后残留 DB）时，网格经 `onError` 兜底回退原片，不出现裂图。
- [ ] AC11 删除项目时清理该项目对应的磁盘缩略图（或在 design 中明确记录"接受残留并依赖 id 稳定性"的取舍——二选一，需在 design.md 定）。
