# 相似组浏览 UI

> Parent: `05-24-milestone-1-mvp`｜覆盖 ROADMAP M1 功能 ⑦相似组浏览

## Goal

前端按"组"渲染照片：列出各相似组，组内默认按 `blur_score` / 拍摄时间排序，让用户快速扫到每组里谁更值得保留。是评审环节的主视图，承接 A/B 对比与状态标记的入口。

## Scope

### In Scope

1. 新增 query command（如 `list_groups` / `list_photos_by_group`），Rust 从 DB 读组 + 组内照片（含分析分数、status）。
2. 前端 Zustand store 持有组数据；按组分块渲染（长列表用 TanStack Virtual）。
3. 图片显示走 Tauri `convertFileSrc`（前端不直接读磁盘）。
4. 组内默认排序：`shot_at` 升序（拍摄时间优先），`blur_score` 降序做次键、`id` 兜底。
5. 组内每张展示关键标记（is_blurry / exposure_flag / 当前 status），并提供进入 A/B 对比与切换 status 的挂载点。
6. 样式栈引入 Tailwind + shadcn/ui（本项目首个 UI 任务，定调 frontend/component-guidelines 的样式 OPEN 决策）；UI/视觉设计走 `ui-ux-pro-max`。
7. 启用 Tauri `assetProtocol`（前置阻塞，见下）。

### Out of Scope

- A/B 对比 viewer 本体 → ab-compare（本任务只留入口）。
- status 变更的落库逻辑 → keep-reject-status（本任务只调用其 command / 留挂载点）。
- 缩略图缓存（M2）；M1 直接用 `convertFileSrc` 原图或浏览器缩放。

## 决策点（已定）

- **组内排序**：`shot_at` 升序优先（连拍原始顺序），`blur_score` 降序做次键、`id` 兜底。
- **未分组单张**：独立"未分组"区，置于所有相似组之后。`group_members` 本就不含连通分量=1 的照片，所以这些照片（含未分析/分析失败）单独查（`photos LEFT JOIN group_members WHERE group_id IS NULL`）。
- **样式栈**：Tailwind + shadcn/ui，定调项目级 OPEN 决策。
- **store 分区**：延续既有 per-domain 模式，新增 `groupsStore`（与现有 `photosStore` 并列）。

## 前置阻塞（必须在本任务解决）

- **Tauri `assetProtocol` 未启用**：`tauri.conf.json` 的 CSP 已放行 `asset:`，但 `app.security.assetProtocol` 缺 `enable:true` + `scope`。不补这段，`convertFileSrc` 生成的 `asset://` URL 无法加载，网格里所有图都是空白。M1 取舍：`scope` 用宽松值（本地单用户桌面应用），收紧到导入根目录列为后续。

## 软依赖

需 similar-grouping 的组结构 + analysis-subsystem 的分数（均已落库，见 `0002`/`0003` migration）。可先用 mock 数据搭 UI，`list_groups` 命令就绪后接真数据。

## Acceptance Criteria

- [ ] 导入并分析+分组后，UI 按相似组展示，组内按 `shot_at` 升序（`blur_score` 降序次键）排序；未分组单张归入末尾"未分组"区。
- [ ] 数百张照片滚动流畅（TanStack Virtual 生效，单一滚动容器扁平化虚拟化组头+缩略图行），不一次性渲染全部 DOM。
- [ ] 图片经 `convertFileSrc` 加载且 `assetProtocol` 已启用，网格能真实显示图片（非空白）。
- [ ] HEIC 在网格先占位（带文件名+标记），真实解码路径留给 ab-compare（D5）。
- [ ] 每张可见 status pill（pending/keep/reject，形状+文字+色，非仅颜色）与 is_blurry / exposure_flag 标记；提供进入 A/B 对比与切换 status 的挂载点（status 落库属 keep-reject-status，本任务做 optimistic store + 调用挂载点）。
- [ ] 键盘可达：缩略图可聚焦、可用键盘进入对比/切状态。
- [ ] 三层 formatter（rustfmt+clippy / ruff / prettier+tsc）通过。
