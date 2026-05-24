# Implement — Milestone 0 项目骨架

> 顺序执行；每个阶段结尾的"验证"必须通过才能进入下一阶段。
> 三个 deliverable 之间正交，原则上失败时只需回滚当前阶段。

## Stage A — Tauri / 前端 / Rust 工程骨架

A1. 写仓库根 `package.json`（scripts: `dev` / `build` / `tauri`；deps: `react`, `react-dom`, `@tauri-apps/api`；devDeps: `@tauri-apps/cli`, `@vitejs/plugin-react`, `typescript`, `vite`, `@types/react`, `@types/react-dom`）。

A2. 写 `vite.config.ts`、`tsconfig.json`、`tsconfig.node.json`、`index.html`、`src/main.tsx`、`src/App.tsx`、`src/api/echo.ts`、`src/styles.css`。

A3. 写 `src-tauri/Cargo.toml`（deps: `tauri = "2"`, `tauri-build = "2"` (build), `serde`, `serde_json`, `tokio` with `rt-multi-thread`/`macros`/`process`/`io-util`/`sync`/`time`, `rusqlite = { version = "0.31", features = ["bundled"] }`, `chrono`。**不**引 `thiserror` / `anyhow`，spec OPEN 未定）。

A4. 写 `src-tauri/tauri.conf.json`、`src-tauri/build.rs`、`src-tauri/src/main.rs`（仅 bootstrap）、`src-tauri/src/lib.rs` 的"Hello world"版（先不集成 sidecar / db，只让窗口能打开）。

A5. 验证：
- `npm install` 无错。
- `npm run tauri dev` 成功打开窗口，前端显示骨架页（无 echo 按钮也行，能跑就过）。

## Stage B — Python sidecar + Rust 调用 + echo 跑通

B1. 写 `python/pyproject.toml`（uv 管理；`requires-python = ">=3.10"`；本里程碑 deps 留空，echo 用标准库）+ `python/main.py`（stdin 行循环，dispatch by op）+ `python/analyzers/__init__.py` + `python/analyzers/echo.py`（`def run(payload: dict) -> dict`，spec 要求的分析器签名）。

B2. 在 `python/` 跑 `uv sync`（生成 `.venv`、`uv.lock`）。

B3. 写 `src-tauri/src/sidecar/mod.rs`：
- `Sidecar` 结构持有 `Mutex<ChildStdin>`、pending oneshot map、`AtomicU64 next_id`。
- 启动函数 `spawn_dev()`：dev 期 `Command::new("uv").args(["run","python","main.py"]).current_dir(<python 绝对路径>)`，stderr 继承（透传到 Rust stderr）。
- reader task 独占 stdout，逐行解析 JSON，按 id 路由到 oneshot；解析失败 eprintln! 到 stderr 但不中止 loop。
- `call(op, payload) -> Result<Value, Box<dyn Error...>>` 实现 5s 超时。

B4. 写 `src-tauri/src/error.rs` 的最小 `AppError` enum（手写 `serde::Serialize` `#[serde(tag, content)]`）+ `src-tauri/src/commands/mod.rs` 中 `echo_via_sidecar(text, state) -> Result<String, AppError>`。

B5. 在 `lib.rs` 里：
- `tauri::Builder` 的 `.setup()` 中 spawn sidecar 并 `manage(AppState { sidecar })`；spawn 失败用 eprintln! 警告但 setup 继续返回 Ok（让 UI 还能显示错误状态）。
- `.invoke_handler(generate_handler![echo_via_sidecar])`。
- `main.rs` 调用 `photo_picker_lib::run()`。

B6. 在 `src/App.tsx` 增加 echo 按钮 + 调用 `api/echo.ts` + 显示返回；在 `src/api/echo.ts` 包 `invoke('echo_via_sidecar', { text })`。

B7. 验证：
- `npm run tauri dev` 起来后点按钮，UI 显示 `sidecar replied: hello sidecar`。
- 关掉 sidecar 进程后再点，UI 显示 `sidecar unavailable: ...`（手工 kill 验证）。

## Stage C — SQLite + 初始 migration

C1. 写 `src-tauri/migrations/0001_initial.sql`（内容见 design §4.1，仅 `photos` 表 + 索引；**无独立 version 表**）。

C2. 写 `src-tauri/src/db/mod.rs`：
- `open(app_handle) -> Result<Connection, Box<dyn Error>>`：拼 `app_handle.path().app_data_dir()? / "photo-picker.db"`，确保父目录存在，开 WAL + synchronous=NORMAL + foreign_keys=ON。
- `run_migrations(conn) -> Result<(), Box<dyn Error>>`：`PRAGMA user_version` 读 current；对 `MIGRATIONS[current..]` 顺序执行；每段用事务包，结束 `PRAGMA user_version = N`。
- `const MIGRATIONS: &[&str] = &[include_str!("../../migrations/0001_initial.sql")];`。
- 返回的 `Connection` 用 `std::sync::Mutex` 包了塞进 `AppState`（M0 不立即使用，先 manage 起来）。

C3. 修改 `lib.rs`：`.setup()` 中先 `db::open` + `db::run_migrations`，失败 panic（spec：失败 migration 中止 boot）。

C4. 验证（Windows）：
- `npm run tauri dev` 启动后到 `%APPDATA%\<identifier>\photo-picker.db` 用 `sqlite3` 查 `SELECT name FROM sqlite_master WHERE type='table';` 看到 `photos`。
- `PRAGMA user_version;` 返回 `1`。
- `PRAGMA journal_mode;` 返回 `wal`；`PRAGMA synchronous;` 返回 `1`；`PRAGMA foreign_keys;` 返回 `1`。

## Stage D — 收尾

D1. 三层 formatter check：
- `npx prettier --check .`
- `(cd src-tauri && cargo fmt --check)`
- `(cd python && uv run ruff format --check .)`

D2. 勾上 `ROADMAP.md` 三个 checkbox：
- `[x] Tauri 2 + React + TS 工程骨架可启动`
- `[x] Rust 主进程能调起 Python sidecar 跑通一次 echo`
- `[x] SQLite 建库 + 初始 migration`

D3. 走 trellis-check：spec 合规、跨层数据流、reuse、一致性。

D4. `task.py done` → `archive`。

## 回滚点

| 触发条件 | 回滚动作 |
| --- | --- |
| Stage A 失败 | 删 `package.json` / `vite.*` / `src/` / `src-tauri/`，回到初始仓库 |
| Stage B 失败但 A 通过 | 删 `src-tauri/src/sidecar.rs` 和 `commands.rs`、`python/main.py`、回退 `main.rs` 到 A 末态 |
| Stage C 失败但 B 通过 | 删 `src-tauri/src/db.rs`、`migrations/`、`<app_data_dir>/photo-picker.db`，回退 `main.rs` 到 B 末态 |

## 验收命令一览

```bash
# 启动
npm install
(cd python && uv sync)
npm run tauri dev          # 手工：点 echo 按钮看回显

# 格式化检查
npx prettier --check .
(cd src-tauri && cargo fmt --check)
(cd python && uv run ruff format --check .)

# DB 验证（Windows，替换 <id> 为 tauri.conf.json 里的 identifier）
sqlite3 "$APPDATA/<id>/photo-picker.db" ".tables"
sqlite3 "$APPDATA/<id>/photo-picker.db" "PRAGMA user_version;"
sqlite3 "$APPDATA/<id>/photo-picker.db" "PRAGMA journal_mode;"
```
