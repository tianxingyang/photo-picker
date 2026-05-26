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

## 已确认事实（代码勘察 2026-05-26）

- **资产协议前置项 — 已解决** ✅：group-browse-ui(#5) 已在 `tauri.conf.json` 启用 `app.security.assetProtocol = { enable: true, scope: ["**"] }`，CSP 也已放行 `img-src ... asset: http://asset.localhost data: blob:`。JPEG/PNG 经 `convertFileSrc` 已能在 webview 渲染（group-browse 网格已验证）。本任务不再需要动资产协议配置。
- **HEIC 仍是占位** ：`src/components/browse/HeicPlaceholder.tsx` 注释明写「Real decode path is deferred to ab-compare (D5)」；`BrowsePhoto.isHeic` 标志已存在。D5 在本任务真正落地。
- **sidecar 已能解码 HEIC**：`python/pyproject.toml` 已依赖 `pillow-heif`；分析器走 JSON-Lines over stdio，各 `analyzers/<op>.py` 暴露 `run(payload)->dict`（Rust 侧 `src-tauri/src/sidecar/` 管理）。→ 加一个「转码出临时 JPEG/PNG」的 op 与现有架构同构，复用已有依赖。
- **挂载点已就绪**：`src/App.tsx` 的 `onCompare(id: PhotoId)` 现为占位（弹提示）；group-browse 的 `GroupBrowseView`/`PhotoCard` 经 `onCompare` 上抛点击。本任务负责把 viewer 接到这个 seam，并定义「打开后展示哪两张 + 怎么切」。
- **栈**：React18 + TS + Vite + Tailwind3 + zustand + clsx/tailwind-merge；暗色单主题，语义 token（`--primary #7C3AED`、`--keep/--reject/--pending`、`--surface` 等）已在 group-browse 落地，本任务沿用。

## 决策点

- **D5 HEIC 显示路径 — 已锁定：sidecar 按需转码** ✅（用户 2026-05-26 决定）。复用 Python `pillow-heif`，新增 `transcode` op：HEIC → 临时 JPEG/PNG，按 `源路径 + mtime` 派生缓存键写入 OS temp，命中即复用；前端拿临时路径走已验证的 `convertFileSrc` 显示。零新依赖、与 JSON-Lines sidecar 架构同构。临时文件生命周期与失败回退在 design.md 细化。与 M2「批量缩略图 + WebP 缓存」边界清晰：此处是 lazy/按需/只转当前对比的 2 张、保留可缩放高保真。
- **布局 — 已锁定：左右并排** ✅（用户 2026-05-26 决定）。两图左右并排，缩放（滚轮/按钮）与平移（拖拽）锁定同步；对应键盘 `1`=左、`2`=右。叠加/闪烁切换属 M2+。
- **打开/导航 seam — 已锁定：App 级全屏 overlay + 组内胶片条** ✅（用户 2026-05-26 决定）。点击网格瓦片打开 overlay；点击张=A、默认 B=同组下一张；底部胶片条展示整组缩略图，点击设 B、可 A↔B 互换、方向键在组内步进 B；`1`/`2` 给 A/B 打 `keep`（直接调已就绪的 `groupsStore.setStatus`，落库走已完成的 keep-reject-status）；Esc 关闭。`onCompare(id)` 现有单 id 签名不变——viewer 由 id 反查 `groupsStore` 取所属组；孤立/未分组照片打开后 B 为空槽、给「无同组照片可对比」提示。
- **打开/导航 seam**：viewer 如何被打开、默认对比哪两张、如何换另一张（点击张 + 同组下一张？组内胶片条选两张？）。`onCompare` 现仅传单个 id，需定 pair 来源与挂载形态（App 级 overlay/modal）。

## 软依赖

近乎独立。只需两张图 URL + 选择回调，可用本地两张测试图独立开发，无需等其它任务。

## Acceptance Criteria

- [ ] 网格点击瓦片 → 打开全屏 overlay，左=点击张(A)、右=同组下一张(B)；非同组成员不出现在胶片条。
- [ ] 缩放（滚轮/按钮）与平移（拖拽）在两图间锁定同步：操作任一图，另一图同步变换；切换 pair 时变换归位。
- [ ] 底部胶片条点击缩略图把该张设为 B；A↔B 可互换；方向键 ←/→ 在组内步进 B；当前 A/B 在胶片条有非纯色高亮（描边+角标）。
- [ ] 键盘 `1`=保留左(A)、`2`=保留右(B)，并有等效屏幕按钮；按下后对应照片 `status` 落库为 `keep`（调 `groupsStore.setStatus`，乐观更新），pane 上状态标即时反映；按 D4 只改当前张、不联动同组。
- [ ] HEIC 两张也能正常显示对比（落实 D5：sidecar 转码）；转码进行中显示 loading 占位（>300ms 用 skeleton/shimmer，非空白）；转码失败回退到 `HeicPlaceholder` 并提示。
- [ ] 大图（数千万像素）对比缩放/平移顺滑、不内存爆（变换只用 transform/opacity；HEIC 转码产物按 `maxSide` 上限约束分辨率）。
- [ ] Esc 关闭 overlay；overlay 打开时 grid 不可被误操作；关闭后焦点回到原触发瓦片（焦点管理）。
- [ ] 可访问性：所有交互键盘可达、有可见 focus ring；图片有 `alt`（文件名）；状态/选择不靠纯色（图标+文字+色）；尊重 `prefers-reduced-motion`。
- [ ] 三层 formatter 通过（Rust `cargo fmt`、Python `ruff`、前端 `prettier`）。
