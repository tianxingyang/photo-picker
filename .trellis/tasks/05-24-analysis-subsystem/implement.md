# 分析子系统 — Implementation Plan

> 前置：`prd.md` 决策已锁定、`design.md` 契约已定。实现严格按下面顺序，每段带校验。
> 全程禁项（`quality-guidelines.md`）：Rust 无 `unwrap/expect/println!`（测试除外）、不跨 `.await` 持 Connection / sidecar 外层锁；Python 无 `print()`（污染 IPC）、日志走 stderr。

## 阶段 A — Schema（先打地基）

- [ ] A1 新建 `src-tauri/migrations/0002_analysis.sql`（六分析列 + `analysis_state`/`analysis_error` + 两索引，见 design §3）。
- [ ] A2 `src-tauri/src/db/mod.rs`：`MIGRATIONS` 数组追加 `include_str!("../../migrations/0002_analysis.sql")`。
- [ ] A3 加 Rust 迁移测试：fresh DB 建出新列；只跑 0001 的 v1 旧库升级后存量行 `analysis_state='pending'`。
- 校验：`cd src-tauri && cargo test db`、`cargo test scanner`（确认 0001 既有测试不回归）。
- 回滚点：本阶段仅新增文件 + 一行数组改动，`git checkout` 即回退。

## 阶段 B — Python 分析器（可独立于 Rust 验证）

- [ ] B1 `python/pyproject.toml` 加依赖 `pillow / pillow-heif / numpy / imagehash`，dev 加 `pytest`；`uv lock`。
- [ ] B2 `python/analyzers/constants.py`：阈值常量（design §6）。
- [ ] B3 `python/analyzers/{exif,blur,exposure,phash}.py`：各暴露纯函数（`extract_shot_at` / `score` / `score` / `compute`），不 catch、不写 stdout。
- [ ] B4 `python/analyzers/analyze.py`：`register_heif_opener()`；`run(payload)` 解码一次 → 归一灰度 → 调四算法 → 合并 camelCase dict（design §5）。
- [ ] B5 `python/main.py`：`OPS["analyze"] = analyze.run`（保留 `echo`）。
- [ ] B6 `python/tests/`：合成 fixture + 用例（design §8 Python 项）。
- 校验：`cd python && uv run pytest`、`uvx ruff format --check . && uvx ruff check .`。
- 风险：blur_score 随分辨率漂移 → 必须先归一尺寸再算（B4 顺序）；imagehash phash 默认 hash_size=8（16 hex），勿误配。

## 阶段 C — Rust 调度（连接 B 与 schema）

- [ ] C1 新建 `src-tauri/src/commands/analysis.rs`：`AnalyzeSummary` / `AnalysisResult`(de) / `analyze_pending` 命令 / `persist_analysis` 纯函数（design §4）。
- [ ] C2 `commands/mod.rs` 导出 `pub mod analysis;`；`lib.rs` `generate_handler!` 注册 `commands::analysis::analyze_pending`。
- [ ] C3 单测：`persist_analysis` 的 Ok/Err 两路径（in-memory DB）；pending 选择查询只取 `pending`。
- 校验：`cd src-tauri && cargo test`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`。
- 风险：sidecar `Arc` 必须短锁取出后释放再 `.await`（照搬 `echo_via_sidecar`）；每张 DB 写各自 `spawn_blocking`，勿把 Connection 带进 `.await`。

## 阶段 D — 全链路验收（dev sidecar）

- [ ] D1 `cargo tauri dev` 起应用；导入含 JPG/PNG/HEIC + 清晰/虚焦 + 过曝/欠曝 + 有/无 EXIF 的 fixture 目录。
- [ ] D2 触发 `analyze_pending`（临时按钮或 devtools `invoke`），核对 `photos` 表：`analysis_state='done'`、六列合理、HEIC 不报错、坏文件落 `failed`+`analysis_error` 且不 panic、再次调用幂等不重算。
- [ ] D3 比对 `prd.md` Acceptance Criteria 逐条打勾。
- 校验：三层 formatter 全绿（`cargo fmt --check`、`cargo clippy -D warnings`、`uvx ruff format/check`）。

## 阶段 E — 文档 / spec 同步（Phase 3.3，提交前）

- [ ] E1 `ARCHITECTURE.md`：§IPC op 枚举 4→`analyze` + 结果形状；§数据流·分析 改单 op + `analyze_pending`。
- [ ] E2 `.trellis/spec/backend/error-handling.md`：`status=analysis_failed` → `analysis_state='failed' + analysis_error`。
- [ ] E3 `.trellis/spec/backend/database-guidelines.md`：Schema Shape OPEN 标注 M1 锁定宽表。
- [ ] E4 `.trellis/spec/backend/directory-structure.md`：注明 `analyze` op 由 `analyze.py` 编排四算法 module。

## 校验命令汇总

```bash
# Rust
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test
# Python
cd python && uvx ruff format --check . && uvx ruff check . && uv run pytest
```

## 风险文件 / 回滚点

- **不改** `src-tauri/src/sidecar/mod.rs`（只调用 `call`）；若发现需改，回 Plan 评估而非顺手改。
- migration forward-only：0002 一旦合并入库不可改内容，需修正另写 0003。
- `python/main.py` 仅加 `analyze` 映射、保留 `echo`；全链路验收通过前不删 `echo`/`echo.py`。
- 每阶段独立可回退（git）；阶段 A/B 互不依赖，可并行实现、最后 C 汇合。

## 提交前 checklist（对齐 `quality-guidelines.md` §Code Review）

- [ ] 新 op `analyze` 已写进 ARCHITECTURE §IPC + analyzer module + Rust 反序列化 + 双侧测试。
- [ ] migration 已对 fresh DB 和 v1 旧库各跑一次。
- [ ] 无 Connection / sidecar 锁跨 `.await`。
- [ ] 新命令返回 `Result<_, AppError>`。
- [ ] 三层 formatter + lint 全绿。
