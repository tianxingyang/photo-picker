# 分析子系统 EXIF/模糊/曝光/pHash

> Parent: `05-24-milestone-1-mvp`｜覆盖 ROADMAP M1 功能 ③EXIF ④模糊 ⑤曝光 + ⑥的 pHash 计算

## Goal

建立 Rust 分析调度 + Python sidecar 分析器的完整子系统：对待分析照片批量派发 `exif/blur/exposure/phash` 四个 op，结果回 Rust 落库。这是 M1 唯一一处"算每张照片的分析值"的地方，为剔废与分组提供数据。

> 把四个分析器合并在一个任务，是因为它们共用同一套 sidecar op 调度、rayon 批处理、结果落库 schema 与进度处理；拆开会让"第一个分析器"被迫扛下全部共享基建。

## Scope

### In Scope

1. **schema（D1 已锁定：宽表）**：新增 migration 给 `photos` 加分析列：`shot_at / blur_score / is_blurry / exposure_score / exposure_flag / phash`（最终列名以本任务 design 为准）。
2. **Rust 调度**：取 `status='pending'` 且未分析的行，按 batch 经 sidecar 派发；rayon/并发控制；结果写回 DB。M1 可在扫描后手动触发或自动触发（择一，写进 design）。
3. **Python 分析器**（各自 `analyzers/<op>.py::run`）：
   - `exif`：`Pillow.getexif()` 取拍摄时间，落 `shot_at` ISO8601；无 EXIF 返回空。
   - `blur`：拉普拉斯方差等（D2）→ `blur_score` + `is_blurry`。
   - `exposure`：灰度均值 + 高低光削顶比 → `exposure_score` + `exposure_flag`。
   - `phash`：16-bit pHash（供 similar-grouping 消费）。HEIC 经 `pillow-heif` 解码。
4. sidecar `op` 枚举扩展 `blur/exposure/phash/exif`（ARCHITECTURE L111）。

### Out of Scope

- 近重复分组逻辑（时间窗口 + 汉明聚类）→ similar-grouping。
- 阈值的 UI 可调面板（M2+）；本任务先硬编码 + 留参数位。
- 分析进度可视化事件（M2）。

## 决策点

- **D1 宽表 vs 高表 — 已锁定：宽表** ✅（2026-05-24）。
- **D2 模糊/曝光阈值策略**（待定）：模糊用拉普拉斯方差 vs Sobel vs FFT；全局阈值 vs 组内归一；曝光阈值标定。建议 M1 先硬编码常量 + 预留可调入参。

## 软依赖

需要 import-scan 产出的 `photos` 行作为输入；但可用一个 fixture 图片目录 + 手写几行 seed 数据独立开发与验证，不阻塞。

## Acceptance Criteria

- [ ] migration 升版（`PRAGMA user_version` +1），新列建出，旧库可平滑升级。
- [ ] 对 fixture 集（含清晰/虚焦、正常/过曝欠曝、有/无 EXIF、含 HEIC）跑分析，四类结果正确落库。
- [ ] 明显虚焦图 `is_blurry=true`；明显废曝光 `exposure_flag` 命中；清晰正常图不误杀。
- [ ] 有 EXIF 的图 `shot_at` 为 ISO8601；HEIC 能被 pillow-heif 解码分析不报错。
- [ ] sidecar 对未知 op / 坏文件返回 `{id, error}`，Rust 侧不 panic、记录并跳过该张。
- [ ] 三层 formatter 通过。
