# Milestone 1 MVP 选片闭环

## Goal

交付 `ROADMAP.md` Milestone 1 锁定的 10 项功能，形成端到端可用的选片闭环：
**导入文件夹 → 自动剔除废片（模糊/曝光）→ 近重复分组 → 组内浏览与 A/B 对比 → 标记保留/淘汰/待定 → 导出精选原片**。

本任务是 **parent**，不直接承载实现，负责：持有跨子任务的共享契约与锁定决策、对齐各子任务边界、做最终端到端集成验收。具体实现在 7 个子任务中独立完成。

## 子任务地图

| 子任务目录 | 覆盖功能 | 主要层 | 软依赖 |
| --- | --- | --- | --- |
| `05-24-import-scan` | ①格式 ②文件夹导入 | Rust | 无（产出 `photos` 行） |
| `05-24-analysis-subsystem` | ③EXIF ④模糊 ⑤曝光 + ⑥的 pHash 计算 | Rust+Python | import-scan 产出的行（可用 fixture 独立验证） |
| `05-24-similar-grouping` | ⑥pHash 近重复分组 | Rust | analysis 落库的 `phash` + `shot_at`（可用种子数据验证） |
| `05-24-group-browse-ui` | ⑦相似组浏览 | 前端 | grouping 的组 + analysis 的分数 |
| `05-24-ab-compare` | ⑧A/B 对比 | 前端 | 仅需两张图 URL（近乎独立） |
| `05-24-keep-reject-status` | ⑨保留/淘汰/待定 | Rust+前端 | `photos.status` 已存在（近乎独立） |
| `05-24-export-selection` | ⑩导出精选 | Rust+前端 | 只读 `status=keep`（近乎独立） |

> parent/child 不是依赖系统。上表「软依赖」只说明数据先后关系；每个子任务都能用 fixture / 种子数据独立 plan-implement-check-archive。真正的执行顺序写在各子任务自己的 `prd.md` / `implement.md` 里。

## 共享契约与锁定决策（Source of Truth）

继承自 M0（M1 不可推翻）：

| 决策 | 取值 |
| --- | --- |
| `photos.status` 类型 | `TEXT` 枚举 `'pending' \| 'keep' \| 'reject'`，带 `CHECK` 约束 |
| 迁移机制 | `PRAGMA user_version` 驱动，`migrations/000N_*.sql` 追加，下标 +1 即版本号 |
| IPC 协议 | JSON-Lines over stdio：`{id, op, payload}` → `{id, result}` / `{id, error}`（ARCHITECTURE.md L89-111） |
| 分析器签名 | Python `analyzers/<op>.py` 暴露 `def run(payload: dict) -> dict` |
| 原片只读 | 全程只读路径引用，绝不修改；仅导出时复制 |
| Python 环境 | 一律 `uv` 管理 |

parent 持有、需在对应子任务 **planning 阶段** 敲定（拆分阶段不预先锁死）：

- **D1 数据模型形状 — 已锁定：宽表** ✅（用户 2026-05-24 决定）。给 `photos` 加 `shot_at / blur_score / is_blurry / exposure_score / exposure_flag / phash` 列（最终列名以 analysis-subsystem design 为准）；M3 的 CLIP embedding / 人脸走独立表，不绑死宽表。
- **D2 模糊/曝光阈值策略**（硬编码 / 用户可调 / 组内归一）— analysis-subsystem 决定；建议 M1 先硬编码 + 留参数位。
- **D3 相似分组时间窗口宽度 + 汉明距离阈值** — similar-grouping 决定。
- **D4 组内联动 — 已锁定：不联动** ✅（用户 2026-05-24 决定）。设 `keep` 只改当前张，同组其余保持原状态（默认 `pending`），由用户手动逐张淘汰；不做自动 reject。
- **D5 HEIC 在 webview 的解码/显示路径** — ab-compare 决定（分析侧读取走 pillow-heif 已定）。

子任务敲定 D1–D5 后，回填本表对应行，保持单一事实源。

## 跨子任务验收（端到端，集成阶段）

- [ ] 7 个子任务全部 `completed` 并归档。
- [ ] 在真实连拍文件夹（含 JPG/PNG/HEIC、含明显虚焦/废曝光、含多组近重复）跑通完整链路。
- [ ] 导入后 `photos` 表含全部文件行，HEIC 也被识别。
- [ ] 分析完成后每张有 `shot_at`（EXIF 存在时）/ `blur` / `exposure` 结果；明显废片被标出。
- [ ] 近重复连拍聚到同一组，组数与肉眼判断大致吻合。
- [ ] 组内可浏览、可 A/B 对比、键盘 1/2 选保留。
- [ ] 保留/淘汰/待定状态落库，重启应用后保持。
- [ ] 导出后目标目录出现且仅出现 `status=keep` 的原片副本，文件名保留，源文件未被改动。
- [ ] `ROADMAP.md` Milestone 1 的 10 项功能全部可用。

## Out of Scope（M1 明确不做）

- 人脸识别、构图评分、CLIP embedding、DBSCAN、云同步、批量编辑（属 M3）。
- 缩略图后台批量预生成 + WebP 缓存、时间线视图、进度可视化、撤销/重做（属 M2）。
- PyInstaller 打包、CI、自动更新（属 M4）。

## Notes

- 各子任务的 `design.md` / `implement.md` 在其自身 planning 阶段补齐；本 parent 只维护契约与集成验收。
- 子任务敲定 D1–D5 任一项后，更新本文件「共享契约」表。
