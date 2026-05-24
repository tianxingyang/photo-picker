# 分析子系统 EXIF/模糊/曝光/pHash

> Parent: `05-24-milestone-1-mvp`｜覆盖 ROADMAP M1 功能 ③EXIF ④模糊 ⑤曝光 + ⑥的 pHash 计算

## Goal

建立 Rust 分析调度 + Python sidecar 分析器的完整子系统：对待分析照片逐张派发单个 `analyze` op（一次解码、内部跑 exif/blur/exposure/phash 四算法），结果回 Rust 落库。这是 M1 唯一一处"算每张照片的分析值"的地方，为剔废与分组提供数据。

> 把四个分析器合并在一个任务，是因为它们共用同一套 sidecar op 调度、批处理、结果落库 schema 与进度处理；拆开会让"第一个分析器"被迫扛下全部共享基建。

## Scope

### In Scope

1. **schema（D1 已锁定：宽表）**：新增 migration 给 `photos` 加分析列：`shot_at / blur_score / is_blurry / exposure_score / exposure_flag / phash` + 状态列 `analysis_state / analysis_error`（最终列名/类型以本任务 design 为准）。
2. **Rust 调度（D-conc/D-trigger）**：`analyze_pending` 命令取 `analysis_state='pending'` 行，经单 sidecar **串行**逐张派发 `analyze`，结果写回 DB，返回 `{analyzed, failed}`。
3. **Python 分析器**（合并为单个 `analyze` op，内部一算法一 module `analyzers/<name>.py`，一次解码）：
   - `exif`：`Pillow.getexif()` 取拍摄时间，落 `shot_at` ISO8601；无 EXIF 返回空。
   - `blur`：拉普拉斯方差（numpy 卷积，归一尺寸后）→ `blur_score` + `is_blurry`。
   - `exposure`：灰度均值 + 高低光削顶比 → `exposure_score` + `exposure_flag`。
   - `phash`：`imagehash.phash`（供 similar-grouping 消费）。HEIC 经 `pillow-heif` 解码。
4. sidecar `op` 新增 `analyze`（替代文档中 `blur/exposure/phash/exif` 4 枚举，保留 `embed`/`face` 预留），同步改 ARCHITECTURE §IPC。

### Out of Scope

- 近重复分组逻辑（时间窗口 + 汉明聚类）→ similar-grouping。
- 阈值的 UI 可调面板（M2+）；本任务先硬编码 + 留参数位。
- 分析进度可视化事件（M2）。

## 决策点

- **D1 宽表 vs 高表 — 已锁定：宽表** ✅（2026-05-24）。
- **D2 模糊/曝光算法与依赖 — 已锁定：轻量栈 + 全局硬编码阈值** ✅（2026-05-24）。依赖 `Pillow + pillow-heif + numpy + imagehash`（不引 opencv，对 M4 去 Python 化友好）。blur=拉普拉斯方差（numpy 卷积，先归一到固定尺寸再算以稳定阈值）；exposure=灰度均值 + 高/低光削顶比；阈值为集中常量 + 允许 payload 覆盖（留参数位）。**组内归一不可行**（分组在分析之后），M1 用全局绝对阈值。
- **D-op IPC op 形状 — 已锁定：单个 `analyze` op** ✅（2026-05-24）。Python 端只解码一次像素，依次调 `blur.py/exposure.py/phash.py/exif.py` 四个内部函数，返回合并结果；每张图 1 次 IPC + 1 次落库。内部仍保持「一个 analyzer 一个 module」以便单测。代价：偏离 ARCHITECTURE §IPC 现记的 4-op 枚举，需同步改文档（op 改为 `analyze`，保留 `embed`/`face` 预留）。
- **D-conc 并发模型 — 已锁定：单 sidecar 串行（MVP）** ✅（2026-05-24）。沿用现有单子进程，Rust 顺序派发 `analyze` 并陆续落库；Rust 调度抽象成可替换接口，进程池并行化留给 M2 性能轮。
- **D-state 分析状态跟踪 — 已锁定：独立 `analysis_state` 枚举 + `analysis_error`** ✅（2026-05-24）。migration 加 `analysis_state TEXT CHECK(pending|done|failed) DEFAULT 'pending'` + `analysis_error TEXT`。调度查 `WHERE analysis_state='pending'`；成功→done、失败→failed+记 `analysis_error`；重试由 failed→pending。与 `status`（用户保留决策）彻底解耦。顺带修正 `error-handling.md` 中与枚举冲突的 `status=analysis_failed` 写法。
- **D-trigger 触发方式 — 已锁定：显式 `analyze_pending` 命令** ✅（2026-05-24）。Rust 命令分析全部 `analysis_state='pending'` 行，跑完返回 `{analyzed, failed}`；触发时机（扫描后自动调 / 按钮）由 UI 任务决定。本任务不做进度事件（M2）。

## 软依赖

需要 import-scan 产出的 `photos` 行作为输入；但可用一个 fixture 图片目录 + 手写几行 seed 数据独立开发与验证，不阻塞。

## Acceptance Criteria

- [ ] migration 升版（`PRAGMA user_version` +1），新列建出（含 `analysis_state` 默认 `pending`），旧库升级后存量行自动变 `pending`、可被分析。
- [ ] `analyze_pending` 命令分析所有 `analysis_state='pending'` 行，跑完返回 `{analyzed, failed}`；成功行 `analysis_state='done'`，再次调用不重复分析（幂等）。
- [ ] 对 fixture 集（含清晰/虚焦、正常/过曝欠曝、有/无 EXIF、含 HEIC）跑分析，四类结果正确落库（单个 `analyze` op 一次解码产出全部字段）。
- [ ] 明显虚焦图 `is_blurry=1`；明显废曝光 `exposure_flag` 命中（`over`/`under`）；清晰正常图不误杀。
- [ ] 有 EXIF 的图 `shot_at` 为 ISO8601；无 EXIF 返回空且不报错；HEIC 能被 pillow-heif 解码分析不报错。
- [ ] 坏文件/未知 op：sidecar 返回 `{id, error}`，Rust 侧不 panic，将该行置 `analysis_state='failed'` + 记 `analysis_error`，继续分析其余张。
- [ ] blur_score 在同图缩放下稳定（归一尺寸生效）。
- [ ] 三层 formatter 通过（`cargo fmt` + `clippy -D warnings`、`uvx ruff format/check`）。
