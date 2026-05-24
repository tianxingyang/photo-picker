# 相似组浏览 UI

> Parent: `05-24-milestone-1-mvp`｜覆盖 ROADMAP M1 功能 ⑦相似组浏览

## Goal

前端按"组"渲染照片：列出各相似组，组内默认按 `blur_score` / 拍摄时间排序，让用户快速扫到每组里谁更值得保留。是评审环节的主视图，承接 A/B 对比与状态标记的入口。

## Scope

### In Scope

1. 新增 query command（如 `list_groups` / `list_photos_by_group`），Rust 从 DB 读组 + 组内照片（含分析分数、status）。
2. 前端 Zustand store 持有组数据；按组分块渲染（长列表用 TanStack Virtual）。
3. 图片显示走 Tauri `convertFileSrc`（前端不直接读磁盘）。
4. 组内默认排序：`blur_score` / 时间（具体次序写进 design）。
5. 组内每张展示关键标记（is_blurry / exposure_flag / 当前 status），并提供进入 A/B 对比与切换 status 的挂载点。

### Out of Scope

- A/B 对比 viewer 本体 → ab-compare（本任务只留入口）。
- status 变更的落库逻辑 → keep-reject-status（本任务只调用其 command / 留挂载点）。
- 缩略图缓存（M2）；M1 直接用 `convertFileSrc` 原图或浏览器缩放。

## 决策点

- 组内排序键与次序（blur 升/降序、时间先后）。
- 单张未分组照片的展示位置（独立区 / 各自成组）。

## 软依赖

需 similar-grouping 的组结构 + analysis-subsystem 的分数。可先用 mock 数据搭 UI，接口就绪后接真数据。

## Acceptance Criteria

- [ ] 导入并分析+分组后，UI 按组展示，组内按既定键排序。
- [ ] 数百张照片滚动流畅（虚拟列表生效），不一次性渲染全部 DOM。
- [ ] HEIC 也能在网格显示（依赖 ab-compare 敲定的 D5 解码路径，或本任务先占位）。
- [ ] 每张可见 status / 废片标记，能触发进入对比与切换状态的入口。
- [ ] 三层 formatter 通过。
