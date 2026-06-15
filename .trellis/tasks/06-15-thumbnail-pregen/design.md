# Design — 缩略图后台批量预生成 + WebP 缓存

> 本文件是技术设计。需求与验收见 `prd.md`，执行清单见 `implement.md`。

## 1. 总览与边界

新增一条与现有 analyze 批次同构的流水线阶段 `thumbnail`，把"为当前项目所有照片生成 512px WebP 缩略图"做成一个 Tauri 命令 `generate_thumbnails`，由前端在 `scan → thumbnails → analyze → group` 链中、analyze 之前 await 调用。几乎所有地基复用现有模式，新增面集中在六处：

| 层 | 新增/改动 | 复用自 |
|---|---|---|
| Migration | `0005_thumbnails.sql`（加 3 列 + 1 索引） | `0002_analysis.sql`（additive ADD COLUMN） |
| Python sidecar | `python/analyzers/thumbnail.py` + 注册 op | `python/analyzers/transcode.py` |
| Rust storage | `db::thumbnails_dir(app)` + 单照片路径推导 | `db::db_path`、`transcode_for_display` 的 dest 逻辑 |
| Rust command | `generate_thumbnails` + `cancel_thumbnails` + AppState 双标志 | `analyze_pending` / `cancel_analysis` |
| Progress | `PHASE_THUMBNAIL` 常量；前端 `PHASES`/标签 | `pipeline://progress` 既有事件 |
| Frontend | `BrowsePhoto.thumbSrc`；网格 + filmstrip 改用缩略图；App 链路插一步 | `displayApi`/`useDisplaySrc` 的 derived-file→asset-url 先例 |

**不改** `tauri.conf.json`（assetProtocol scope `["**"]` + CSP 已允许 `app_data_dir` 下的 WebP）。**不改** `scanner`（见 §4 自愈设计）。**不改** `ComparePane`（大图保持全分辨率/transcode）。

## 2. 数据契约

### 2.1 DB schema（migration 0005，user_version 4→5）

```sql
ALTER TABLE photos ADD COLUMN thumb_status TEXT NOT NULL DEFAULT 'pending'
    CHECK (thumb_status IN ('pending','done','failed'));
ALTER TABLE photos ADD COLUMN thumb_src_mtime INTEGER;   -- 生成时源文件 mtime(nanos)，NULL=从未生成
ALTER TABLE photos ADD COLUMN thumb_error TEXT;          -- 最近一次失败信息
CREATE INDEX idx_photos_thumb_status ON photos(thumb_status);
```

- 镜像 `analysis_state` 的 pending/done/failed 生命周期。纯加列，**不 DROP/重建**（0004 重建仅因改 PK）。
- 在 `src-tauri/src/db/mod.rs` 的 `MIGRATIONS` 常量数组追加 `include_str!("../../migrations/0005_thumbnails.sql")`。测试断言 `user_version == MIGRATIONS.len()`，故数组长度与版本号自动锁步——两半必须同时落地（风险见 §7）。

### 2.2 缩略图磁盘布局

- `thumbnails_dir(app) = app_handle.path().app_data_dir()?.join("thumbnails")`（新 helper，紧挨 `db::db_path`）。
- 单照片 dest = `thumbnails_dir/<id[0:2]>/<id>.webp`，`id = blake3(project_id + '\n' + path)`（64 hex，已天然项目隔离，两级目录防单目录爆炸——对齐 `ARCHITECTURE.md:163`）。
- **就地覆写**：重生成写同一 `<id>.webp`，**不产生孤儿文件**（不在文件名嵌 mtime；mtime 自愈走 §4）。写入用 sidecar 的原子 `.part`→`os.replace`。

### 2.3 Sidecar IPC: `thumbnail` op

请求（Rust→Python，JSON-Lines）：`{id, op:"thumbnail", payload:{path, dest, maxSide:512, quality:80}}`
响应成功：`{id, result:{dest, width, height}}`；按文件错误：`{id, error:"<msg>"}`（Python 内 `raise`，`main.handle` 包装）。沿用 `call()` 两级结果契约（transport err vs per-file err，`sidecar/mod.rs:115-141`）。

### 2.4 Progress 事件（无新事件）

复用 `pipeline://progress` 的 `ProgressEvent{phase,done,total,status}`。新增 `PHASE_THUMBNAIL = "thumbnail"`（`commands/progress.rs`）。前端 `progressStore.ts` 的 `ProgressPhase` 类型与 `PHASES` 数组必须加 `"thumbnail"`（`isProgress()` 硬拒未知 phase，否则事件被静默丢弃），`PipelineProgressBar.tsx` 加中文标签 "生成缩略图"。

### 2.5 前端 DTO

`BrowsePhoto` 增 `thumbSrc?: PhotoSrc | null`（`src/types/photo.ts`）。后端 `load_browse_model`/`read_photo`（`grouping.rs`）投影 `thumb_status`，并在 `thumb_status='done'` 时返回缩略图**绝对路径** `thumb_path`（前端拿不到 `app_data_dir`，路径必须后端给）。`groupsApi.toBrowsePhoto()`：`thumbSrc = raw.thumbPath ? convertFileSrc(raw.thumbPath) : null`。

## 3. 控制流：`generate_thumbnails`

结构克隆 `analyze_pending`（`commands/analysis.rs:76-220`）：

1. 单飞 guard：`thumbnails_running` AtomicBool `compare_exchange` + RAII `RunGuard`（独立于 analysis，**绝不复用** analysis 标志——风险见 §7）。`thumbnails_cancel` 在批次开始置 false。
2. `project_id = current_project(&state)?`。
3. **Stat 过滤 pass**（无 sidecar、无 decode）：`SELECT id, path, thumb_status, thumb_src_mtime FROM photos WHERE project_id=?1`，对每行 `stat(path)` 取 `cur_mtime`，按下表判定是否入工作集：

   | thumb_status | 入工作集条件 |
   |---|---|
   | `pending` | 总是 |
   | `done` | `!dest.exists()` 或 `thumb_src_mtime != cur_mtime`（**mtime 自愈**） |
   | `failed` | `thumb_src_mtime != cur_mtime`（仅源文件变动才重试，避免坏文件每轮重试） |

   `total = 工作集大小`（进度条总数准确）。源文件已不存在 → 跳过并计入 skipped。
4. **生成 pass**：`futures::stream::iter(worklist).buffer_unordered(n)`，第 i 项路由 `pool[i % n]`（`sidecar_pool()`，全池 fan-out）。每项：
   - 开始前查 `thumbnails_cancel`；置位则保持 `pending`、计 skipped。
   - `create_dir_all(dest 的两级父目录)`（仿 `transcode_for_display` `photos.rs:209`）。
   - `sidecar.call("thumbnail", {path, dest, maxSide:512, quality:80})`。
   - 成功：`spawn_blocking` UPDATE `thumb_status='done', thumb_src_mtime=cur_mtime, thumb_error=NULL WHERE id=?1`。
   - 按文件错误：UPDATE `thumb_status='failed', thumb_error=?2 WHERE id=?1`。
   - 共享 `AtomicUsize` done 计数 → `progress::running(app, PHASE_THUMBNAIL, done, Some(total))`（乱序并行下单调）。
5. 终态 tick：`progress::terminal`，状态优先级 error > cancelled > done。

`cancel_thumbnails` 命令置 `thumbnails_cancel`，镜像 `cancel_analysis`。两命令注册进 `lib.rs` 的 `generate_handler!`。

## 4. mtime 自愈（为何不碰 scanner）

scanner 现为 `INSERT OR IGNORE INTO photos`（`scanner/mod.rs:81,246`）：**已存在的 (project_id,path) 行被忽略，不更新、不记 mtime**。因此"靠 rescan 清理陈旧缩略图"在当前模型下根本不触发。结论：**自愈逻辑全部放进 `generate_thumbnails` 的 stat 过滤 pass**（§3.3），不改 scanner。

- 代价：每次生成运行对项目内每张照片做一次 `stat`（无 decode）。万级照片约数十毫秒，可接受；且 auto-chain 在每次 scan 后跑，"无变化"场景下 stat pass 命中全 done、立即终态，不调 sidecar。
- 收益：源文件 mtime 变化、缩略图被手动删除、上次失败后文件被替换——三种情况都能在下次生成时自愈，无需触碰 immutable-import 的 scanner 语义。

## 5. 触发编排（前端链路）

`App.tsx` 当前：`scanFolder`(78) → `analyzePending`(104) → `groupPhotos`(110)。插入一步：

```
scanFolder → await generateThumbnails() → analyzePending → groupPhotos
```

- 新 `src/api/thumbnailsApi.ts`：`generateThumbnails()` invoke `"generate_thumbnails"`；`cancelThumbnails()` invoke `"cancel_thumbnails"`（接入既有 pipeline 取消 UI）。
- `await` 顺序执行 ⇒ 缩略图与分析在 N-sidecar 池上**天然互斥**，杜绝 2N 超订（风险见 §7）。
- 网格在缩略图阶段就能逐步出图（`thumbSrc ?? src`），分析尚未开始。

## 6. 前端渲染改动

- `PhotoCard.tsx`：`src={photo.thumbSrc ?? photo.src}`，并加 `onError` 回退 `photo.src`（缩略图文件缺失时不裂图，AC10）。
- `CompareFilmstrip.tsx` 的 `Thumbnail`（64×64，当前最浪费）：同样改用 `thumbSrc`。
- **HEIC**：分支由 `isHeic ? <HeicPlaceholder> : <img original>` 改为 `thumbSrc ? <img thumb> : isHeic ? <HeicPlaceholder> : <img original>`——缩略图就绪后 HEIC 首次在网格/filmstrip 显示真实预览（FR7）。
- `ComparePane.tsx`：**不动**，大图继续用全分辨率/transcode 图（AC9）。
- **EXIF 朝向**：浏览器对原片 JPEG 默认按 EXIF orientation 旋转显示；缩略图 WebP 会丢 EXIF，故 `thumbnail.py` 在 resize 前 `ImageOps.exif_transpose(img)`，保证缩略图与原片朝向一致（否则旋转过的照片缩略图会躺倒）。`transcode.py` 暂不动。

## 7. 风险与缓解

- **版本锁步**：加 `0005` 文件却漏 `include_str!`（或反之）会同时破坏 boot 迁移与隔离测试。两半同一提交落地，跑 `cargo test` 验证 `user_version==MIGRATIONS.len()`。
- **复用 analysis 标志 = 正确性 bug**：`thumbnails_running`/`thumbnails_cancel` 必须独立新增；复用会让"取消分析"误杀缩略图（反之亦然，AC8）。
- **池超订**：若缩略图与分析并发，各自 `buffer_unordered(n)` 打同 N 个进程 ⇒ 2N in-flight，sidecar 串行化 stdin/stdout 会排队拖慢甚至触发 30s `CALL_TIMEOUT`。靠 §5 的 await 顺序消除。
- **thumb_status 与磁盘漂移**：DB='done' 但文件被删 ⇒ 网格指向缺失 asset。stat 过滤 pass 已用 `!dest.exists()` 兜回（下次生成会补），前端 `onError` 再兜一层。
- **WebP 编码是净新 sidecar 工作**：`thumbnail.py` 需正确处理 `convert('RGB')`（丢 alpha，对挑片可接受）与 `exif_transpose`。Pillow WebP 插件已装，无新依赖（C1）。
- **删除项目残留**（AC11 取舍，**本设计选"删除时清理"**）：`delete_project`（`projects.rs:126-158`）级联删 DB 行但不删磁盘文件。在级联前 `SELECT id FROM photos WHERE project_id=?`，对每个 id `remove_file(dest)`（best-effort，忽略 NotFound），再走原删除。重扫移除照片的孤儿缩略图为次要泄漏，本期接受（与 immutable-import 模型一致）。

## 8. 兼容性与回滚

- **向前兼容**：迁移纯加列，旧数据 `thumb_status` 默认 `pending`，首次链路自动补齐。前端 `thumbSrc` 可空，未就绪时 `?? src` 完全等价当前行为。
- **回滚**：迁移前向只增、无 down-migration（遵循 `database-guidelines.md`）。功能级回滚 = 前端不调 `generate_thumbnails` 且渲染回退 `photo.src`；遗留的 thumb 列与磁盘缓存无害。代码级回滚 = revert PR 分支。
- **数据安全**：仅在 `app_data_dir/thumbnails` 写派生文件，绝不动原片；原片仅读。
