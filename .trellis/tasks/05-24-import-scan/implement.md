# Implement — 导入与扫描管线

执行顺序：先打通 Rust 后端（可独立测），再接前端。每个 checkpoint 后跑对应验证。

## 步骤

### A. 依赖与脚手架

- [ ] A1. `src-tauri/Cargo.toml` 增依赖：`walkdir = "2"`、`blake3 = "1"`、`time = { version = "0.3", features = ["formatting"] }`、`tauri-plugin-dialog = "2"`；`[dev-dependencies]` 增 `tempfile = "3"`。
- [ ] A2. 前端：`npm i zustand @tauri-apps/plugin-dialog`。
- [ ] A3. `cargo build`（在 `src-tauri/`）+ `npm run build` 各跑一次，确认依赖解析通过。

### B. Rust 后端

- [ ] B1. `error.rs`：`AppError` 增 `Db(String)` / `Io(String)` / `NotFound(String)`，补 `Display` 各臂。
- [ ] B2. `lib.rs`：`AppState._db: Mutex<Connection>` → `db: Arc<Mutex<Connection>>`；setup 里 `db: Arc::new(Mutex::new(conn))`。注册 `.plugin(tauri_plugin_dialog::init())`。
- [ ] B3. 新建 `scanner/mod.rs`：实现 `scan_folder(conn: &Connection, root: &Path) -> Result<Vec<PhotoRow>, Box<dyn Error + Send + Sync>>`，含 `PhotoRow`（`#[derive(Serialize)]`）、`now_iso8601()` 助手、扩展名常量。算法见 design §3。用 `prepare_cached` + 单事务。
- [ ] B4. 新建 `commands/photos.rs`：`#[tauri::command] async fn scan_folder(path, state) -> Result<Vec<PhotoRow>, AppError>`，校验 `is_dir`，clone Arc + `spawn_blocking` + `blocking_lock`（见 design §4），错误 `map_err` 成 `AppError`。
- [ ] B5. `commands/mod.rs` 加 `pub mod photos;`；`lib.rs` 的 `generate_handler!` 增 `commands::photos::scan_folder`（保留 echo）。
- [ ] B6. `scanner/mod.rs` 写 `#[cfg(test)] mod tests`（见 design §8），含回查 status 用例。

验证：`cargo test`（scanner 测试绿）、`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`。

### C. 前端

- [ ] C1. `types/photo.ts`：`PhotoStatus`、品牌 `PhotoId` / `PhotoSrc`、`Photo`。`types/ipc.ts`：`AppErrorPayload`（`Sidecar|Db|Io|NotFound`）+ 一个 `describeAppError(e): {kind?, message}` 归一化助手。
- [ ] C2. `api/dialogApi.ts`：`pickFolder(): Promise<string | null>` 包 `open({ directory: true, multiple: false })`。
- [ ] C3. `api/photosApi.ts`：内部 `PhotoRow` 原始类型 + 浅校验（`Array.isArray` + 每项 `typeof/in`），映射成 `Photo`（`name=basename`、`src=convertFileSrc(path)`）；`scanFolder(path)` 校验失败抛 typed error。
- [ ] C4. `store/photosStore.ts`：Zustand，state `{ byId: Record<PhotoId,Photo>; order: PhotoId[] }`，actions `addPhotos(Photo[])` / `clear()`；selector 友好（见 state-management.md）。
- [ ] C5. `App.tsx`：导入视图——`[导入文件夹]` → `pickFolder` →（非空）`scanFolder` → `addPhotos` → 显示「已导入 N 张」+ `name` 列表；`busy` 防抖；错误按 `kind` 提示。移除 echo 按钮（保留 `api/echo.ts`）。

验证：`npx prettier --check .`、`npm run build`（tsc 严格通过）。

### D. 端到端手验

- [ ] D1. `npm run tauri dev`，点导入，选一个含子目录 + 混合 JPG/PNG/HEIC + txt/mp4 的真实文件夹。
- [ ] D2. `sqlite3 <app_data_dir>/photo-picker.db "SELECT count(*),status FROM photos GROUP BY status;"`：仅图片入库、HEIC 在内、全 pending。
- [ ] D3. 再次导入同目录：行数不变（增量幂等）。
- [ ] D4. 抽查一行：`id` 为 64 位 hex，`created_at` 为 ISO8601。

## 验证命令汇总

```bash
# Rust（在 src-tauri/）
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings

# 前端（仓库根）
npx prettier --check .
npm run build

# 端到端
npm run tauri dev
```

## 回滚点

- 纯增量：删除新增的 `scanner/`、`commands/photos.rs`、前端新文件，还原 `error.rs` / `lib.rs` / `App.tsx` / `Cargo.toml` / `package.json` / `capabilities/default.json` 即回到 M0。
- 无 migration，DB 无结构变更；已写入的 `photos` 行删库文件即清。

## Review Gate（开工前确认项）

1. 新增依赖（walkdir/blake3/time/plugin-dialog/zustand）OK？
2. `AppState._db` → `Arc<Mutex<Connection>>` 重构 OK？
3. import-scan 不渲染图片、只显示文件名（把 HEIC 显示留给 D5/ab-compare）OK？
4. echo 按钮从 UI 移除、command 保留 OK？
