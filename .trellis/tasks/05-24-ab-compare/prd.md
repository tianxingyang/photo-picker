# A/B 对比 Viewer

> Parent: `05-24-milestone-1-mvp`｜覆盖 ROADMAP M1 功能 ⑧A/B 对比

## Goal

自研双图对比组件：左右（或叠加）展示两张照片，缩放/平移同步联动，键盘 `1`/`2` 选择保留哪张。是从一组近重复里二选一的精修利器。架构上近乎独立——只需要两张图的可显示 URL + 一个"选择"回调。

## Scope

### In Scope

1. `ABCompareViewer` 组件：接收两张照片（id + 可显示 URL）。
2. 同步缩放（滚轮/按钮）+ 同步平移（拖拽），两图变换锁定一致。
3. 键盘 `1` 选左图保留、`2` 选右图保留，触发回调（落库交给 keep-reject-status）。
4. 适配大图：用 `convertFileSrc`，避免把原图塞进 base64。

### Out of Scope

- 组数据加载与组内导航 → group-browse-ui（本组件被它挂载/调用）。
- status 落库 → keep-reject-status（本组件只 emit 选择事件）。
- 多图（>2）对比、差异高亮（M2+）。

## 决策点

- **D5 HEIC 在 webview 的显示路径**：WebView2/WebKit 不一定原生渲染 HEIC。需定：sidecar 转码出临时 JPEG/PNG 供显示，还是其它方案。本任务敲定并回填 parent PRD（影响 group-browse-ui 的 HEIC 显示）。
- 布局：左右并排 vs 叠加切换。

> ⚠️ **资产协议前置项（import-scan 评审 #4 发现）**：`photosApi.toPhoto` 已用 `convertFileSrc(path)` 生成 `Photo.src`（`http://asset.localhost/...`），但 import-scan 阶段 **从不渲染图片**，所以该 URL 至今未生效。本任务（或 group-browse-ui，谁先渲染图片谁负责）在 `<img src>` 之前**必须**：① 在 `tauri.conf.json` 加 `app.security.assetProtocol = { enable: true, scope: [...] }`——仅 CSP 写 `asset:` **不会**注册该协议处理器；② scope 需覆盖用户导入的任意 OS 路径（导入目录是动态的，可能需运行时用 `tauri-plugin-fs` 的 scope API 动态授权，而非静态配置）。否则所有图片 403/加载失败。这一项推翻了 import-scan design §7「CSP 已够、无需改」的判断。

## 软依赖

近乎独立。只需两张图 URL + 选择回调，可用本地两张测试图独立开发，无需等其它任务。

## Acceptance Criteria

- [ ] 给两张图，缩放一张另一张同步缩放、平移同步联动。
- [ ] 按 `1`/`2` 触发对应选择回调，参数含被选照片 id。
- [ ] 大图（数千万像素）对比不卡顿、不内存爆。
- [ ] HEIC 两张也能正常显示对比（落实 D5）。
- [ ] 三层 formatter 通过。
