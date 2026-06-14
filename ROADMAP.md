# Roadmap

## Milestone 0 — 项目骨架

- [x] 技术选型确认
- [x] 约束文件与文档（README / ARCHITECTURE / ROADMAP / 各 formatter）
- [x] Tauri 2 + React + TS 工程骨架可启动
- [x] Rust 主进程能调起 Python sidecar 跑通一次 echo
- [x] SQLite 建库 + 初始 migration

## Milestone 1 — MVP（用户已锁定的 10 项功能）✅ 2026-06-13 交付

| # | 功能 | 关键细节 / 决策点 |
| - | - | - |
| 1 | JPG / PNG / HEIC 支持 | HEIC 依赖 `pillow-heif`；Rust 扫描层只看扩展名 |
| 2 | 文件夹导入 | 递归 walkdir；导入后增量索引，不重复哈希 |
| 3 | EXIF 时间读取 | Pillow.getexif()，落 `shot_at` ISO8601 字符串 |
| 4 | 模糊检测 | **决策点**：拉普拉斯方差 vs Sobel vs FFT；全局阈值还是组内归一 |
| 5 | 曝光检测 | 灰度均值 + 高低光削顶比；阈值待标定 |
| 6 | pHash 近重复分组 | 16-bit phash + 时间窗口 + 汉明距离阈值 |
| 7 | 相似组浏览 | UI 按组渲染，组内默认按 blur_score / 时间排序 |
| 8 | A/B 对比 | 双图同步缩放/平移、键盘 1/2 选保留 |
| 9 | 保留 / 淘汰 / 待定 | **决策点**：组内联动策略；状态变更要写 DB 持久化 |
| 10 | 导出精选 | 复制（不移动）keep 状态的原片到目标目录，保留原文件名 |

(MVP 范围内不实现：人脸识别、构图评分、CLIP embedding、云同步、批量编辑)

## Milestone 2 — 体验增强

- [x] 项目级工作区隔离（projects 表 + 按项目作用域）— 交付 2026-06-14（#10/#11）
- [x] 导入 / 分析 / 分组进度可视化（前端订阅 Rust 事件，顶部细长条）— 交付 2026-06-15
- [x] 分析多核并行（Rust 启 N 个同步 sidecar 进程池，analyze 轮询分发）+ 协作式取消 — 交付 2026-06-15
- 缩略图后台批量预生成 + WebP 缓存
- 时间线视图（按拍摄时间轴聚合）
- 快捷键全覆盖（导航、状态切换、对比）
- 撤销 / 重做（保留-淘汰的操作历史）

## Milestone 3 — 智能化

- CLIP / MobileCLIP embedding 接入，相似分组从 pHash 升级到语义相似
- DBSCAN 聚类替代纯阈值
- 人脸检测（MediaPipe）：同一人脸归一组，便于"同动作"对比
- 表情/睁眼检测：辅助挑出"睁眼正脸"那张

## Milestone 4 — 工程化与分发

- Python sidecar → ONNX Runtime + Rust/C++ 实现，去 Python 依赖
- GitHub Actions：Windows / macOS / Linux 三平台自动构建与签名
- 自动更新（Tauri updater）
- 崩溃上报（可选，需用户授权）

## 长期演进

- 移动端（iOS / Android）：复用 Rust 核心 + 平台原生 UI；分析层全 ONNX Runtime。
- 与外部修图软件（Lightroom / Capture One）的"已选标记"互通。
- 个性化偏好学习：根据用户历史保留行为，给推荐评分。

## 决策待办池

下列决定推迟到对应任务开工时再定，但已经记录在此避免遗漏：

- [x] 数据模型：宽表 vs 高表（影响后续所有 SQL）— **宽表**（M1 D1，2026-05-24）
- [x] 状态字段：TEXT 枚举 vs INT 编码 — **TEXT 枚举 + CHECK 约束**（M0 锁定）
- [x] 模糊阈值：硬编码 vs 用户可调 vs 组内归一 — **M1 硬编码 + 留参数位**（M1 D2）
- [x] 相似分组的时间窗口宽度 — **不用时间窗口**，纯 pHash 汉明距离连通分量（M1 D3，2026-05-25）
- [x] 组内 keep 时是否自动 reject 其它 — **不联动**，手动逐张淘汰（M1 D4，2026-05-24）
- [ ] 缩略图分辨率档位（一档 256? 双档 256/512?）（M2 开工时定）
- [x] HEIC 解码是否走 sidecar 还是 Rust 端用 libheif crate — **sidecar `pillow-heif` 按需转码 + 临时缓存**（M1 D5，2026-05-26）
