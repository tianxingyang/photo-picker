# Architecture

## 进程拓扑

```
┌──────────────────────────────────────────────┐
│                Tauri Window                  │
│  ┌────────────────────────────────────────┐  │
│  │  Frontend (React + TS + Vite)          │  │
│  │   - Zustand stores                     │  │
│  │   - TanStack Virtual grid              │  │
│  │   - A/B compare viewer                 │  │
│  └────────────┬───────────────────────────┘  │
│               │ @tauri-apps/api invoke       │
│  ┌────────────▼───────────────────────────┐  │
│  │  Rust Main Process                     │  │
│  │   - #[tauri::command] handlers         │  │
│  │   - walkdir 扫描 + blake3 ID           │  │
│  │   - rusqlite + tokio::spawn_blocking   │  │
│  │   - rayon 并行调度分析任务             │  │
│  │   - sidecar manager (stdin/stdout)     │  │
│  └────────────┬───────────────────────────┘  │
│               │ JSON-Lines over stdio        │
│  ┌────────────▼───────────────────────────┐  │
│  │  Python Sidecar (PyInstaller bundle)   │  │
│  │   - OpenCV / Pillow / imagehash        │  │
│  │   - 模糊 / 曝光 / pHash / EXIF         │  │
│  │   - 后续：CLIP embedding + DBSCAN      │  │
│  └────────────────────────────────────────┘  │
└──────────────────────────────────────────────┘
```

## 各层职责

### Frontend (`src/`)

- 负责 UI、用户交互、可视化反馈。
- **不**直接读取磁盘、不直接打开图片文件；通过 Tauri `convertFileSrc` 拿到可显示的 URL。
- 通过 `api/` 包装 invoke 调用，类型从 `types/` 共享。

### Rust 主进程 (`src-tauri/src/`)

- 文件 I/O、数据库、缓存、与 sidecar 通信的唯一入口。
- 长耗时任务（扫描、哈希）放 `tokio::task::spawn_blocking` 或 rayon thread pool，不阻塞 Tauri 事件循环。
- 通过 `tauri::State` 持有 DB 连接句柄与 sidecar 句柄。

### Python sidecar (`python/`)

- 单一职责：吃图像路径、吐分析结果。
- 不持有状态，不写数据库；所有结果回 Rust 由 Rust 写入 SQLite。
- 通过 PyInstaller 打成单文件，由 Tauri externalBin 机制随主进程启动。

## 数据流：导入文件夹

```
用户点击"导入文件夹"
  └─> Frontend: open() 拿到目录
       └─> invoke('scan_folder', { path })
            └─> Rust: walkdir 扫描，过滤扩展名
                 └─> 每个文件：blake3(path) 生成 id
                      └─> 写入 photos 表（status=pending）
                           └─> 返回基础元数据列表给前端
                                └─> Frontend 渲染 grid
```

## 数据流：分析

```
扫描完成 / 用户手动触发
  └─> Frontend: invoke('analyze_pending')
       └─> Rust: 取出 analysis_state='pending' 的行，单 sidecar 串行逐张派发
            └─> 对每张：调用 sidecar.call("analyze", {path})
                 └─> sidecar 一次解码 → 内部跑 blur/exposure/phash/exif → 返回合并 JSON
                      └─> Rust 增量写入 DB（成功 analysis_state='done'；失败='failed'+analysis_error）
                           └─> 全部跑完返回 {analyzed, failed}（进度事件留待 M2）
```

## 数据流：相似分组

```
所有照片分析完成
  └─> Rust 取出 (id, shot_at, phash) 列表
       └─> 1) 按 shot_at 切时间窗口（如 ±30s）
       └─> 2) 窗口内做 pHash 汉明距离阈值聚类
       └─> 3) (可选) 调 sidecar 求 CLIP embedding + DBSCAN
       └─> 写入 similar_groups 表
            └─> 前端按组展示
```

## 数据流：保留/淘汰/待定状态

```
用户在卡片上点保留 / 淘汰 / 待定
  └─> Frontend: groupsStore.setStatus 乐观写 store（状态标签即时翻）
       └─> invoke('set_status', { photoId, status })
            └─> Rust: 枚举校验（非法值→Validation）
                 └─> spawn_blocking 内单行 UPDATE photos SET status
                      └─> 0 行受影响→NotFound；成功→返回 ()（D4=不联动，只改当前张）
                           └─> 成功保留乐观态 / 失败回滚 prev 并 rethrow（前端 .catch 静默吞）
```

## 数据流：导出精选

```
用户点击"导出精选"
  └─> Frontend: pickFolder() 选目标目录（取消→null→静默返回）
       └─> invoke('export_keep', { destDir })
            └─> Rust export_keep:
                 ① dest.is_dir() 校验（否→Validation）
                 ② spawn_blocking #1（持 DB 锁）：SELECT path FROM photos WHERE status='keep'
                 ③ spawn_blocking #2（不持 DB 锁）：逐张 resolve_target 探空位 → std::fs::copy
                      · 源已在目标目录内（规范化源文件父目录==目标）→skipped++（不复制，避免自我克隆）
                      · 目标重名→ name (n).ext（renamed++）
                      · 成功→exported++；单项失败/源无文件名/源已删除/名额耗尽→failed.push（不中断）
                 └─> 返回 ExportSummary { exported, renamed, skipped, failed:[{source,reason}] }
            └─> Frontend 据 exported/renamed/skipped/failed 拼 notice（0 张→明确提示）；
                failed 明细在可折叠 <details> 列表逐项展示（文件名 — 原因）
```

源原片全程只读：`std::fs::copy` 只读源、写新目标，绝不 rename/remove/写回源路径；`resolve_target`
只在探到「目标不存在」时才写，故不覆盖目标已有同名文件。

## IPC 协议（Rust ↔ Python）

JSON-Lines over stdio，每行一个 JSON 对象。

请求（Rust → Python）：

```json
{ "id": 42, "op": "analyze", "payload": { "path": "C:/photos/IMG_001.jpg" } }
```

响应（Python → Rust）。`analyze` 一次解码产出全部分析字段（camelCase）：

```json
{ "id": 42, "result": {
  "shotAt": "2026-05-24T10:30:00",   // 无 EXIF → null
  "blurScore": 124.7,
  "isBlurry": false,
  "exposureScore": 0.42,              // 归一灰度均值 [0,1]
  "exposureFlag": "normal",           // normal | over | under
  "phash": "ffc3a18000000000"         // 64-bit pHash → 16 hex
} }
```

错误：

```json
{ "id": 42, "error": "FileNotFoundError: ..." }
```

`id` 由 Rust 单调递增，用于多路复用。`op` 当前枚举：`analyze`（Python 端解码一次，依次调
`blur` / `exposure` / `phash` / `exif` 四个内部 module 合并结果），后续可扩展 `embed` / `face`。

## 存储位置

- 数据库：`<app_data_dir>/photo-picker.db`（WAL 模式）。
- 缩略图缓存：`<app_data_dir>/thumbnails/<id[0:2]>/<id>.webp`，两级目录避免单目录文件爆炸。
- 用户原片：应用内部**绝不复制、绝不移动、绝不修改**（缩略图/分析都不把原片复制入库），全程只读路径引用。唯一例外是用户主动发起的「导出精选」——把 `status='keep'` 的原片**只读拷贝**到用户自选目录，源文件零改动（见上「数据流：导出精选」）。

## 关键设计决策（待用户确认）

以下决定会显著影响实现，留到 schema/algo 阶段再定：

1. **状态机的组内联动**：组内某张被设为 `keep` 时，其余张默认 `pending` 还是 `reject`？
2. **数据模型形状**：分析结果用宽表（`photos` 加列）还是高表（独立 `photo_analyses`）？
3. **模糊检测阈值**：硬编码、用户可调、还是按整组动态归一？
4. **相似分组的窗口宽度**：仅时间？时间 + 文件名前缀？时间 + 拍摄设备？

这些决策点在 ROADMAP.md 对应任务里也有交叉引用。
