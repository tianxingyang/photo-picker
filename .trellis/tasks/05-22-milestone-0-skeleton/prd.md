# Milestone 0 项目骨架

## Goal

完成 `ROADMAP.md` Milestone 0 中剩余三项 checkbox，让仓库具备"`npm run tauri dev` 能起、Rust 能调 Python sidecar、SQLite 能建库"的最小可运行骨架，为 Milestone 1 各项功能铺好接入面。

## Scope

### In Scope

1. Tauri 2 + React + TypeScript + Vite 工程骨架，`npm run tauri dev` 能打开窗口、显示首屏。
2. Rust 主进程拉起 Python sidecar 子进程，跑通一次 echo（前端按钮 → `invoke` → Rust → sidecar stdin → Python 回 stdout → Rust → 前端展示）。
3. SQLite 建库 + 初始 migration：首次启动在 `app_data_dir` 创建 `photo-picker.db`（WAL），执行 `0001_initial.sql` 建出 `photos` 与 `schema_version` 表。
4. 三层 formatter 都能 run 通：`prettier`（前端）、`rustfmt`（Rust）、`ruff`（Python）。

### Out of Scope

- Milestone 1 的任何分析逻辑（blur/exposure/phash/EXIF）。
- 完整 photos 表所有列；本里程碑只放 M1 一定会用到的核心列。
- PyInstaller 打包脚本；本里程碑只覆盖开发期运行方式。
- 前端 UI 美化、Zustand store、虚拟列表等 M1+ 关注的能力。
- CI / GitHub Actions（属于 Milestone 4）。

## Constraints

- 开发期 sidecar 用 `uv run python main.py` 启动（用户决策）。打包期改 PyInstaller，本里程碑不实现，只在 Rust 端用 `cfg!(debug_assertions)` 或环境变量留出切换点。
- `photos.status` 字段类型为 `TEXT`，枚举值 `'pending' | 'keep' | 'reject'`（用户决策）。
- Python 环境一律 `uv` 管理；不允许 `pip install` 到全局。
- IPC 协议严格遵循 `ARCHITECTURE.md` 第 89-111 行定义的 JSON-Lines `{id, op, payload}` → `{id, result}` / `{id, error}` 形态。
- 代码风格：精简高效、无冗余；注释/文档非必要不形成。
- 仅做 M0 范围内改动，不预先实现 M1 的任何特性。

## Acceptance Criteria

- [ ] 在仓库根目录执行 `npm install && (cd python && uv sync) && npm run tauri dev` 能打开 Tauri 窗口，显示一个最小首屏。
- [ ] 首屏有一个按钮（如 "Echo test"），点击后 1 秒内显示形如 `sidecar replied: hello sidecar` 的文本，证明 Rust ↔ Python IPC 全链路通。
- [ ] 应用启动时在 `app_data_dir` 下创建 `photo-picker.db`，可通过 `sqlite3` 看到 `photos` 表；`PRAGMA user_version;` 返回 `1`（spec 要求 migration 用 `PRAGMA user_version` 跟踪版本，不另建表）。
- [ ] `photos` 表至少含列：`id TEXT PK`, `path TEXT NOT NULL UNIQUE`, `status TEXT NOT NULL DEFAULT 'pending'`, `created_at TEXT NOT NULL`，并对 `status` 加 `CHECK` 约束限定三值。
- [ ] `PRAGMA journal_mode;` 在文件级返回 `wal`（建库后会出现 `*.db-wal` / `*.db-shm` 文件作为旁证）。`synchronous` 与 `foreign_keys` 是**每连接**设置不写文件头，验证以 Rust 源码 `db::open` 中三条 `pragma_update` 调用为准（spec 要求一致）。
- [ ] 三层格式化命令均能 run 通：`npx prettier --check .`、`cargo fmt --check`（在 `src-tauri`）、`uv run ruff format --check`（在 `python/`）。
- [ ] `ROADMAP.md` Milestone 0 三个未勾选项全部勾上。

## Decisions Locked (Source of Truth)

| 决策 | 取值 | 影响 |
| --- | --- | --- |
| dev 期 sidecar 启动方式 | `uv run python main.py`（cwd=`python/`） | Rust sidecar 模块开发期走该命令；prod 切换点在 `cfg!(debug_assertions)` 分支 |
| `photos.status` 字段类型 | `TEXT` 枚举 `'pending' \| 'keep' \| 'reject'` | 所有后续 SQL / Rust enum / 序列化层都按 TEXT 来 |

## Open Questions (推迟到 Milestone 1)

- 分析结果用宽表 vs 高表（`photos` 加列还是独立 `photo_analyses` 表）。
- HEIC 解码归属 Python sidecar 还是 Rust `libheif` crate。
- 缩略图分辨率档位。
