# Implement — 相似组浏览 UI

> 执行清单。设计见 `design.md`，需求/验收见 `prd.md`。
> 三层 formatter 是验收硬门：`cargo fmt` + `cargo clippy`、`ruff`（本任务不碰 python）、`prettier` + `tsc`。

## 顺序与评审门

### Step 0 — 前置阻塞：assetProtocol（最先做，带验证门）⚠️
- [ ] 改 `src-tauri/tauri.conf.json` `app.security` 加 `assetProtocol: { enable: true, scope: ["**"] }`（CSP 不动）。
- [ ] **验证门 G0**：`npm run tauri dev` 起应用 → 导入一个含普通 jpg/png 的文件夹 → 在临时挂的 `<img src={convertFileSrc(path)}>` 或 devtools 里确认 `asset://` 请求 **200 且图片可见**。
  - 若空白/403：调 scope（`["**/*"]`、Windows 盘符 glob、`requireLiteralLeadingDot:false`），必要时查 `capabilities/default.json` 是否需补 asset 权限。**此门不过，后面 UI 全是空白，不要继续。**
- 回滚点：仅还原 `tauri.conf.json` 一段。

### Step 1 — 后端 `list_groups` 命令
- [ ] 在 `src-tauri/src/commands/grouping.rs` 加 `BrowsePhoto` / `BrowseGroup` / `BrowseModel`（camelCase serde）。
- [ ] 加 `#[tauri::command] pub async fn list_groups`：clone `state.db` → `spawn_blocking` + `blocking_lock`，纯读，不跨 await 持锁。
- [ ] SQL：组成员（JOIN，`method='phash_burst'`）取全量 → Rust 端 group-by + 组内排序（`shot_at` 升、`blur_score` 降、`id`）+ 组间排序（最早 `shot_at` 升）；未分组 `LEFT JOIN ... WHERE group_id IS NULL`。
- [ ] 在 `src-tauri/src/lib.rs` 的 `invoke_handler![]` 注册 `commands::grouping::list_groups`。
- [ ] 单测（仿 grouping.rs 既有 `mem_conn` 套路）：组内排序正确、未分组含单张/未分析/失败、空库返回空。
- [ ] **验证门 G1**：`cargo fmt --manifest-path src-tauri/Cargo.toml` + `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` + `cargo test --manifest-path src-tauri/Cargo.toml` 全绿。

### Step 2 — 前端样式栈（Tailwind + shadcn/ui）
- [ ] 装 `tailwindcss postcss autoprefixer`（devDeps），`npx tailwindcss init -p`，配 `content`。
- [ ] `src/styles.css` 引 `@tailwind base/components/utilities` + §5 语义 token CSS 变量；`index.html` 的 `<html class="dark">`。
- [ ] 初始化 shadcn/ui（`components.json`），按需加 Badge/Button（或自写等价小组件，避免过度引入）。
- [ ] **验证门 G2**：`npm run build`（vite + tsc）通过；起 dev 确认 Tailwind 类生效、暗色背景正确。

### Step 3 — 类型 + api + store
- [ ] `src/types/photo.ts` 加 `ExposureFlag` / `AnalysisState` / `BrowsePhoto` / `BrowseGroup`（`src: PhotoSrc` 铁律）。
- [ ] `src/api/groupsApi.ts`：`listGroups()` + `toBrowsePhoto`（边界校验、`basename`、`convertFileSrc`、`isHeic`），仿 `photosApi.ts`。
- [ ] `src/store/groupsStore.ts`：`byId/groups/ungroupedIds/loaded` + `load()` + 乐观 `setStatus()`（`set_status` 未就绪则只更新本地 + TODO）+ `clear()`。
- [ ] **验证门 G3**：`tsc` 无错；store 选择器按 id（无整 store 读取）。

### Step 4 — 虚拟化 + 组件
- [ ] 装 `@tanstack/react-virtual`。
- [ ] `GroupBrowseView`：`ResizeObserver` 算 `COLS` → `useMemo` 建 `Row[]`（header + photos 切行 + 未分组区）→ `useVirtualizer`（精确 `estimateSize`）→ 单一滚动容器只渲染可见行。
- [ ] 组件：`GroupHeader`、`PhotoRow`、`PhotoCard`（`React.memo`）、`StatusPill`、`QualityBadges`、`CardActions`、`HeicPlaceholder`、`EmptyState`。
- [ ] 交互：瓦片 `tabIndex=0` + `focus-visible:ring`；`Enter`→`onCompare`；状态切换有屏幕按钮（键盘可达），actions 默认半隐但始终可聚焦。
- [ ] 视觉按 §5：StatusPill 三重信号、QualityBadges 图标+文字、`aspect-square object-cover`、`<img loading="lazy">` + 固定容器防 CLS、HEIC 占位。
- [ ] 在 `App.tsx` 接入：提供进入浏览的入口并调 `groupsStore.load()`（导入+分析+分组后）。
- [ ] **验证门 G4**：`npm run build` 通过；`prettier --check` 通过。

### Step 5 — 端到端验收（对 prd.md Acceptance Criteria 逐条核）
- [ ] 真数据：导入 → `analyze_pending` → `group_photos` → 浏览：按组展示、组内 `shot_at` 升序、未分组在末尾。
- [ ] 数百张滚动流畅，DOM 不渲染全部（devtools 看节点数随滚动恒定）。
- [ ] 图片真实显示（G0 已保障 assetProtocol）。
- [ ] HEIC 显示占位瓦片。
- [ ] 每张可见 status pill + 废片/曝光标记；切 status 乐观即时；进 A/B 挂载点可触发。
- [ ] 键盘可达：Tab 到瓦片、Enter 进对比、按钮切状态。
- [ ] 三层 formatter 全绿。

## 验证命令汇总
```bash
# Rust
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test  --manifest-path src-tauri/Cargo.toml
# 前端
npm run build           # vite build = tsc + bundle
npx prettier --check "src/**/*.{ts,tsx,css}"
# 手动
npm run tauri dev       # G0 图片加载、G4 端到端
```

## 风险文件 / 回滚点
- `src-tauri/tauri.conf.json`（assetProtocol；Windows scope 风险最高，G0 死守）。
- `src-tauri/src/lib.rs`（invoke_handler 注册——改错会让命令不可见）。
- `package.json` / lockfile（新增 tailwind/postcss/autoprefixer/@tanstack-react-virtual/shadcn 依赖）。
- 全部为新增/配置改动，无 DB 迁移、无破坏既有命令；回滚 = 还原上述文件 + 删新增文件。

## 完成前 followups
- Phase 3 spec update：回填 `frontend/component-guidelines.md` 样式 OPEN（→Tailwind+shadcn）、`frontend/state-management.md` 分区 OPEN（→per-domain）。
- 与 `keep-reject-status`：确认 `set_status` 命令契约，替换 store 里的 TODO 挂载点。
- 与 `ab-compare`：`onCompare` 真实跳转 + HEIC D5 解码路径。
