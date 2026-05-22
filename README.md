# Photo Picker

桌面端自动选片工具。一次拍摄常常产出几十上百张相似照片，本工具的目标是：

1. 自动剔除虚焦、严重模糊、曝光失败的废片。
2. 把同场景/同动作/同角度的连拍聚成一组，提供 A/B 两两对比。
3. 辅助用户在数百张里快速锁定保留的几张。

## 目标场景

- 家属/人像/亲子的连拍挑选。
- "同动作多角度"或"多组连拍"的快速分组浏览。
- 出片前的初筛：先一键剔除明显废片，再进入人工精修。

## 技术栈

| 层级 | 选型 |
| --- | --- |
| 桌面框架 | Tauri 2 |
| 前端 | React + TypeScript + Vite；状态 Zustand；长列表 TanStack Virtual；自研 A/B compare viewer |
| 主进程 | Rust；并发 tokio + rayon；DB rusqlite (bundled)；遍历 walkdir；哈希 blake3 |
| 数据库 | SQLite（WAL），存于系统 `app_data_dir` |
| 缩略图 | 本地文件夹缓存，WebP/JPEG 小图 |
| 图像分析 (MVP) | Python sidecar：OpenCV / Pillow / pillow-heif / MediaPipe / imagehash / NumPy / scikit-learn（FAISS 可选） |
| IPC | Rust ↔ Python，stdin/stdout JSON-Lines |
| 模型推理 | MVP 用 PyTorch/MediaPipe；后期迁 ONNX Runtime |
| 相似分组 | 时间窗口 + pHash/dHash + CLIP/MobileCLIP embedding + DBSCAN |
| 打包 | Tauri bundler + Python sidecar (PyInstaller)；GitHub Actions 多平台 |

## 目录结构（规划）

```
photo-picker/
├── src/                      # 前端 React 代码
│   ├── components/           # UI 组件（含 A/B compare viewer）
│   ├── store/                # Zustand stores
│   ├── api/                  # Tauri invoke 包装
│   └── types/                # 共享 TS 类型
├── src-tauri/                # Rust 主进程
│   ├── src/                  # commands / db / scanner / sidecar
│   ├── migrations/           # SQL schema
│   └── tauri.conf.json
├── python/                   # 分析 sidecar（用 uv 管理）
│   ├── main.py               # stdin/stdout 协议入口
│   └── analyzers/            # blur / exposure / phash / exif
└── docs/                     # 设计文档（如有）
```

## MVP 功能范围

详见 [ROADMAP.md](./ROADMAP.md)。当前 MVP 锁定 9 项：
JPG/PNG/HEIC 支持、文件夹导入、EXIF 时间、模糊检测、曝光检测、pHash 近重复分组、相似组浏览、A/B 对比、保留/淘汰/待定、导出精选。

## 架构与数据流

详见 [ARCHITECTURE.md](./ARCHITECTURE.md)。

## 开发环境要求

- Node.js ≥ 18，npm（或 pnpm）
- Rust toolchain ≥ 1.77
- Python ≥ 3.10，使用 [`uv`](https://docs.astral.sh/uv/) 管理虚拟环境
- 平台：Win11 自带 WebView2；macOS 需 Xcode CLT；Linux 需 webkit2gtk

## 启动命令（规划）

```bash
# 前端依赖
npm install

# Python sidecar 环境
cd python && uv sync && cd ..

# 开发模式（Tauri 自动拉起 vite）
npm run tauri dev

# 打包
npm run tauri build
```

## 代码风格约束

- TypeScript / JSON / CSS：Prettier，配置见 `.prettierrc`。
- Rust：rustfmt，配置见 `rustfmt.toml`。
- Python：Ruff（format + lint），配置见 `python/ruff.toml`。
- 跨编辑器缩进/换行符由 `.editorconfig` 统一。
- 整体原则：精简高效、毫无冗余；注释与文档非必要不形成。
