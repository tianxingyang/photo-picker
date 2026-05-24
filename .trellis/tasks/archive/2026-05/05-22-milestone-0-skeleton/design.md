# Design — Milestone 0 项目骨架

## 1. 整体拓扑（落到本里程碑）

```
src/                      React + Vite，仅含最小 App + 一个 echo 按钮
└─ api/echo.ts            wrap `invoke('echo_via_sidecar', { text })`

src-tauri/
├─ src/
│  ├─ main.rs             仅 bootstrap：调用 photo_picker::run()
│  ├─ lib.rs              tauri::Builder + manage(AppState) + 注册 commands（spec 要求）
│  ├─ commands/
│  │   └─ mod.rs          #[tauri::command] 入口；目前只有 echo_via_sidecar
│  ├─ db/
│  │   └─ mod.rs          rusqlite 连接 + PRAGMA + run_migrations（PRAGMA user_version 驱动）
│  ├─ sidecar/
│  │   └─ mod.rs          Child + tokio 包的 stdin/stdout，req/resp by id
│  └─ error.rs            最小 AppError enum（serde::Serialize 给前端），不引第三方错误库
├─ migrations/
│  └─ 0001_initial.sql    仅 photos 表（无独立 version 表，按 spec）
└─ tauri.conf.json        externalBin 暂不配，dev 期由 sidecar 模块直接 spawn `uv`

python/
├─ pyproject.toml         uv 管理；deps 仅占位（echo 用标准库）
├─ main.py                stdin 行循环，dispatch by op
└─ analyzers/
    └─ echo.py            def run(payload: dict) -> dict（spec 要求分析器都是这个签名）
```

## 2. 关键接口契约

### 2.1 Rust ↔ Python (JSON-Lines over stdio)

请求行：
```json
{"id": 1, "op": "echo", "payload": {"text": "hello sidecar"}}
```

正常响应：
```json
{"id": 1, "result": {"text": "hello sidecar"}}
```

错误响应：
```json
{"id": 1, "error": "unknown op: xxx"}
```

- `id`：Rust 端 `AtomicU64` 自增，从 1 起，多路复用用。
- 一行一个 JSON 对象，UTF-8，结尾 `\n`。
- M0 只实现 `op="echo"`；M1 再加 `blur` / `exposure` / `phash` / `exif`。

### 2.2 Frontend ↔ Rust (Tauri invoke)

| Command | Args | Returns | Errors |
| --- | --- | --- | --- |
| `echo_via_sidecar` | `{ text: string }` | `string`（sidecar 回的 text） | `string`（人类可读错误） |

## 3. Sidecar 进程管理

- 启动时机：`tauri::Builder::setup` 钩子里 spawn 一次，存入 `AppState`。
- dev 模式（`cfg!(debug_assertions)`）：`Command::new("uv").args(["run","python","main.py"]).current_dir("../python")`。
- prod 模式：留 TODO，本里程碑只 panic + 日志提示，等 Milestone 4 实现 externalBin。
- 进程句柄结构：
  ```rust
  struct Sidecar {
      stdin: Mutex<ChildStdin>,
      pending: Mutex<HashMap<u64, oneshot::Sender<Response>>>,
      next_id: AtomicU64,
  }
  ```
- 一条 reader 任务（`tokio::spawn`）独占 `ChildStdout`，逐行 parse JSON，按 `id` 找到对应 `oneshot::Sender` 投递结果。
- 关闭策略：进程退出时 reader task 自然结束；本里程碑不实现 graceful shutdown（app 关闭 OS 自然回收）。

## 4. DB / Migration

- `rusqlite` 加 `bundled` feature，避免依赖系统 sqlite。
- 连接路径：`AppHandle::path().app_data_dir()`（Tauri 2 API）拼 `photo-picker.db`；目录不存在则创建。
- 打开后立刻（spec/backend/database-guidelines.md §Overview）：
  ```sql
  PRAGMA journal_mode=WAL;
  PRAGMA synchronous=NORMAL;
  PRAGMA foreign_keys=ON;
  ```
- Migration 机制（spec：用 `PRAGMA user_version` 跟踪，不另建表）：
  - Rust hardcode `const MIGRATIONS: &[&str] = &[include_str!("../../migrations/0001_initial.sql")];`，下标 +1 即为版本号。
  - 启动时 `PRAGMA user_version` 读出 `current`；对 `MIGRATIONS[current..]` 顺序事务执行，每执行一段 `PRAGMA user_version = N`。
  - 失败抛错由 `lib::run()` 上层 panic（spec：失败 migration 中止 boot）。

### 4.1 `0001_initial.sql` 内容

```sql
CREATE TABLE IF NOT EXISTS photos (
  id         TEXT PRIMARY KEY,
  path       TEXT NOT NULL UNIQUE,
  status     TEXT NOT NULL DEFAULT 'pending'
             CHECK (status IN ('pending','keep','reject')),
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_photos_status ON photos(status);
```

设计取舍：
- 现在只放 M1 必经的 4 列；EXIF/blur/exposure 等列推迟到 M1 决定"宽表 vs 高表"后再加。
- `id` 用 TEXT 而非 BLOB，方便日志/调试肉眼读（值预期是 `blake3` 的 hex）。
- 不建独立 version 表：spec 明确 `PRAGMA user_version` 驱动迁移，单文件一次版本号 bump。

## 5. 前端最小骨架

- `index.html` + `src/main.tsx` + `src/App.tsx`。
- `App.tsx` 只放：标题 + `<input>` + `<button>` + 显示区。
- `src/api/echo.ts` 包装 `invoke<string>('echo_via_sidecar', { text })`，捕获异常转 `Error`。
- 不引 Zustand、不引路由，M0 不需要。

## 6. 错误处理边界

- spec/backend/error-handling.md 中 Rust 错误库 OPEN，禁止本里程碑钉死 `thiserror` 或 `anyhow`。本任务自定义最小 `AppError` enum（手写 `serde::Serialize` 标签化，无第三方 derive）：
  ```rust
  #[derive(Debug, serde::Serialize)]
  #[serde(tag = "kind", content = "message")]
  pub enum AppError {
      Sidecar(String),
      Db(String),
      Io(String),
  }
  ```
- 命令侧统一 `Result<T, AppError>`；变体的 `kind` 字段是前端 switch 的契约（spec：前端 switch on `kind`）。
- 内部模块按 spec 临时用 `Result<T, Box<dyn std::error::Error + Send + Sync>>`，到 `commands/` 边界 `map_err` 转 `AppError`。
- Sidecar 启动失败：app 不退出，`echo_via_sidecar` 返回 `AppError::Sidecar("not started: <reason>")`，前端读 `kind="Sidecar"` 给出明确提示。
- DB migration 失败：lib::run() panic，原因 eprintln! 到 stderr；首次启动数据库即异常无降级。

## 7. 兼容性与回滚

- 删除 `<app_data_dir>/photo-picker.db` 即等于完整回滚 M0 的所有 DB 痕迹。
- 删除 `node_modules`、`src-tauri/target`、`python/.venv` 即清空全部构建产物。
- 三个 deliverable 互相独立，任何一项失败不阻塞其他两项；但 echo 依赖 sidecar 必须能起。

## 8. 主要风险

| 风险 | 缓解 |
| --- | --- |
| Windows 上 `uv` 不在 PATH | sidecar 启动失败信息明确写"uv not found in PATH"，README 增补一行依赖说明 |
| Tauri 2 默认 scaffold 与既有目录结构冲突 | 手工搭，不跑 `npm create tauri-app`，避免覆盖已有 README/LICENSE |
| rusqlite bundled 编译需要 C 工具链 | Windows 本地已有 MSVC（Rust toolchain 预装即可）；README 已要求 Rust ≥ 1.77 |
| Python sidecar 不响应导致前端按钮卡死 | echo 命令在 Rust 端加 5s 超时，超时返回错误字符串 |

## 9. 不做（明确）

- 不引入 tracing 框架；M0 用 `eprintln!` 足够。
- 不写自动化测试；M0 验收靠手工运行 + sqlite3 检查。
- 不做跨平台路径修正（除文档提示），M0 仅在 Windows 主开发机走通即视为达标。
