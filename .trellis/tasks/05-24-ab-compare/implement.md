# Implement — A/B 对比 Viewer

> Task: `05-24-ab-compare` ｜ Parent: `05-24-milestone-1-mvp`
> 执行清单。需求见 `prd.md`；技术设计见 `design.md`。
> 顺序：先打通 HEIC 转码后端（可独立验证），再前端 viewer，最后接线 + 端到端。

## 阶段 A — 后端 HEIC 转码（D5，可独立验证）

- [ ] **A1** 新增 `python/analyzers/transcode.py`：`run(payload)->dict`（见 design §2.1）。自包含 `register_heif_opener()`；`.part` 临时名 + `os.replace` 原子落盘；`maxSide` 夹紧；返回 `{dest,width,height}`。
- [ ] **A2** 注册 op：`python/main.py` import 并加入 `OPS["transcode"]`；`python/analyzers/__init__.py` 的 `__all__` 加 `transcode`。
- [ ] **A3** Python 测试 `python/tests/test_transcode.py`：用 fixture（JPEG + 一张小 HEIC，若无 HEIC fixture 用 pillow-heif 写一张）验证：HEIC→可读 JPEG、超 `maxSide` 被降采样、产物尺寸正确、坏路径抛异常（被 main 兜成 error）。
- [ ] **A4** Rust 命令 `transcode_for_display`（`src-tauri/src/commands/photos.rs`，见 design §2.2）：
  - `spawn_blocking` + `blocking_lock` 查 `SELECT path FROM photos WHERE id=?1`；`QueryReturnedNoRows`→`AppError::NotFound`（照搬本文件 set_status 的并发/锁范式）。
  - 缓存键 `blake3(path + "|" + mtime_nanos)` → `std::env::temp_dir()/photo-picker-display/<key>.jpg`；`dest.exists()` 命中即返回。
  - 克隆 sidecar Arc 后释放锁再 `.await`（照搬 `echo_via_sidecar`）；两层 Result：`Ok(Err)`/`Err` 都映射 `AppError::Sidecar`。
  - 返回 `String`（temp 路径）。
- [ ] **A5** 注册命令：`src-tauri/src/lib.rs` `generate_handler!` 加 `commands::photos::transcode_for_display`（line 69 列表内）。
- [ ] **A6** Rust 单测（photos.rs `mod tests`）：缓存键派生稳定/随 mtime 变化；缺失 id → NotFound 路径（可只测纯 helper，sidecar 调用不在单测覆盖）。

> **验证门 A（先于前端）**：`cd python && uv run pytest` 绿；`cd src-tauri && cargo test` 绿；手动用现有 echo 通路或一次 `cargo tauri dev` 中触发转码确认 temp 目录生成 JPEG。

## 阶段 B — 前端 viewer

- [ ] **B1** api 层 `src/api/displayApi.ts`：`transcodeForDisplay(id): Promise<PhotoSrc>`（invoke + 边界校验 + `convertFileSrc`，见 design §3.3）。
- [ ] **B2** store `src/store/compareStore.ts`：`open/memberIds/aId/bId` + `openFor/setB/swap/stepB/close`（design §3.1）；`openFor` 跨读 `useGroupsStore.getState()` 定位组；孤立张 `bId=null`。
- [ ] **B3** hook `src/hooks/useDisplaySrc.ts`：非 HEIC 直返 `photo.src`；HEIC 调 `transcodeForDisplay` 一次 + 模块级 `Map` 缓存，返回 `{state,src}`（design §3.2）。
- [ ] **B4** hook `src/hooks/useHotkey.ts`：`useHotkey(map, enabled)`，绑/解 `keydown`；输入框聚焦时忽略（design §3.4）。
- [ ] **B5** 组件 `src/components/compare/`（design §3.6 / §4）：
  - `ABCompareViewer.tsx`（overlay 容器 + 单一 `view` 状态 + 滚轮/拖拽手柄 + `useHotkey`；焦点陷阱 + 还原）
  - `ComparePane.tsx`（`useDisplaySrc` → `<img>`|`CompareLoading`|`HeicPlaceholder` 回退；`transform` 应用 view）
  - `CompareToolbar.tsx`（关闭、缩放 +/−/重置、N/total+文件名+缩放%）
  - `CompareFilmstrip.tsx`（组缩略图，点击设 B、A/B 角标高亮、互换钮；按 id 选择器订阅）
  - `CompareKeepBar.tsx`（保留左(1)/保留右(2)，图标+文字+色，调 `groupsStore.setStatus(id,'keep')`）
  - `index.ts` 汇出
- [ ] **B6** 视觉：复用 `src/styles.css` 既有语义 token；Lucide 图标（已有 `src/components/browse/icons.tsx`，按需补）；动效 150–300ms、`transform/opacity`、`prefers-reduced-motion`。**视觉走 ui-ux-pro-max 规格（design §4）**。

## 阶段 C — 接线 + 端到端

- [ ] **C1** `src/App.tsx`：渲染 `<ABCompareViewer/>`；`onCompare(id)` 改为 `useCompareStore.getState().openFor(id)`，删占位 `setNotice`。
- [ ] **C2** 自检数据流（design §5）全链路：grid 点击 → overlay → 同步缩放/平移 → 胶片条换 B/方向键 → 1/2 落库 → Esc 关闭+焦点还原。

> **验证门 C（端到端，手动 GUI 由用户驱动）**：见下「手动冒烟」。

## 校验命令（三层 formatter + 类型 + 测试）

```bash
# 前端：类型 + 构建 + 格式
npm run build                       # tsc -b && vite build（类型门）
npx prettier --check "src/**/*.{ts,tsx,css}"
# Python：lint + format + 测试（uv 管理）
cd python && uv run ruff check . && uv run ruff format --check . && uv run pytest
# Rust：format + clippy + 测试
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

## 手动冒烟（GUI，用户驱动 — 给步骤不代跑）

> Windows dev：`npm run tauri dev`（端口 5173；1420 保留）。需一个含 JPG/PNG **和 HEIC** 的真实连拍文件夹。

1. 导入该文件夹 → 分析并分组 → 网格出现相似组。
2. 点某组一张瓦片 → 打开全屏 overlay，左=点击张、右=同组下一张。
3. 滚轮放大左图 → 右图同步放大；拖拽平移 → 两图同步；缩放%随动。
4. 底部胶片条点另一张 → 设为右(B)；点互换 → A/B 调换；按 ←/→ 步进 B。
5. **HEIC 验证**：让 A 或 B 是 HEIC → 先显「解码中」skeleton，随后正常显示可缩放图；故意断/坏一张看回退 `HeicPlaceholder`+提示。
6. 按 `1` 保留左、`2` 保留右 → pane 状态标变 keep；关闭后回网格，对应瓦片 StatusPill = 保留；**重启 app** 后仍保留（落库验证）。
7. 同组其余张状态不变（D4 不联动）。
8. Esc 关闭 → 焦点回到原触发瓦片；overlay 打开时点不到底层 grid。
9. 大图（数千万像素 / HEIC）双开缩放平移顺滑、内存不爆。

## 风险文件 / 回滚点

- **最高风险**：`transcode.py` + `transcode_for_display`（Windows + 真实大 HEIC 的端到端：temp 写权限、`convertFileSrc` 对 temp 路径生效、30s 超时内）。验证门 A 先独立打通再接前端。
- **次高风险**：`ABCompareViewer` 的同步缩放/平移大图性能与内存。
- 回滚：纯新增 + `App.tsx`/`main.py`/`lib.rs`/`__init__.py` 的注册行还原；无 DB 迁移、无破坏性变更（design §7）。

## 完成定义

- 校验命令三层全绿（fmt/lint/type/test）。
- `prd.md` 全部 Acceptance Criteria 勾选（手动冒烟覆盖 GUI 项）。
- 回填 parent PRD D5 行（已在 planning 阶段回填，实现后复核措辞）。
- Phase 3：spec 更新（若 HEIC 显示/转码、overlay/焦点陷阱、useHotkey 等沉淀为约定）+ 经 PR 合入（main 受保护，不直推）。
