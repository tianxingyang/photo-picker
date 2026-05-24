# Design — 导入与扫描管线

> Parent: `05-24-milestone-1-mvp`。本任务是 M1 数据流源头：选目录 → 递归扫描 → 过滤格式 → blake3 ID → 增量写 `photos`。**不触碰 sidecar**（扫描是纯 Rust，ARCHITECTURE §数据流导入）。

## 1. 落到本任务的模块改动

```
src/
├─ App.tsx                 改：echo 测试 → 导入视图（按钮 + 计数 + 文件名列表）
├─ api/
│  ├─ dialogApi.ts         新：pickFolder() 包 plugin-dialog 的 open({directory:true})
│  └─ photosApi.ts         新：scanFolder(path) 包 invoke('scan_folder')，边界校验 + 映射
├─ store/
│  └─ photosStore.ts       新：Zustand，持有导入结果（byId + order）；addPhotos/clear
├─ types/
│  ├─ photo.ts             新：Photo / PhotoStatus / 品牌 PhotoId / PhotoSrc
│  └─ ipc.ts               新：AppErrorPayload 联合（mirror Rust enum 的 kind）

src-tauri/
├─ src/
│  ├─ lib.rs               改：注册 dialog 插件、scan_folder command；AppState.db 改 Arc
│  ├─ commands/
│  │  ├─ mod.rs            改：`pub mod photos;`（echo 留在 mod.rs 或拆出，见 §6）
│  │  └─ photos.rs         新：#[tauri::command] async scan_folder
│  ├─ scanner/
│  │  └─ mod.rs            新：walkdir + 扩展名过滤 + blake3 + 增量 upsert + 回查
│  └─ error.rs             改：AppError 增 Db / Io / NotFound 变体
└─ Cargo.toml              改：+ walkdir, blake3, time, tauri-plugin-dialog
```

**不需要 migration**：`photos(id, path, status, created_at)` 四列 M0 已建，import-scan 不加列（分析列归 analysis-subsystem）。

## 2. 关键契约

### 2.1 Frontend ↔ Rust

| Command | Args | Returns | Errors (kind) |
| --- | --- | --- | --- |
| `scan_folder` | `{ path: string }` | `PhotoRow[]`（见 2.2） | `Io` / `Db` / `NotFound` |

- 同步语义：一次 invoke 走完整扫描，返回该目录下当前全部受支持照片的真实 DB 状态。进度事件属 M2，不做。
- 文件夹选择由前端 `plugin-dialog` 完成后把**绝对路径**字符串传进来；Rust 不弹窗。

### 2.2 数据形状

Rust 返回行（serde 序列化，camelCase 由 serde rename 或前端映射）：

```rust
#[derive(Serialize)]
struct PhotoRow {
    id: String,          // blake3(path) hex
    path: String,        // 绝对路径（OS 原样）
    status: String,      // 'pending' | 'keep' | 'reject'
    created_at: String,  // ISO8601
}
```

前端边界（`photosApi.ts`，type-safety OPEN 未决 → 手写 mirror + 浅校验）：

```ts
// types/photo.ts —— 组件/store 视图，刻意不暴露 raw path
export type PhotoStatus = "pending" | "keep" | "reject";
export type Photo = { id: PhotoId; name: string; status: PhotoStatus; src: PhotoSrc };
```

`photosApi.scanFolder` 把 `PhotoRow` 映射为 `Photo`：`name = basename(path)`、`src = convertFileSrc(path)`（在 api/ 内完成，组件永不见 raw path，符合 frontend/directory-structure.md）。import-scan 列表只显示 `name`，**不渲染图片**（HEIC 显示是 D5，归 ab-compare，不在此提前引入）。

## 3. 扫描算法（scanner/mod.rs）

```
scan_folder(conn, root) -> Result<Vec<PhotoRow>, DbErr>:
  1. walkdir(root)，不跟随 symlink，过滤：
       - entry.file_type().is_file()
       - 扩展名 ∈ {jpg, jpeg, png, heic, heif}（小写比较）
     收集 matched: Vec<PathBuf>
  2. 一个事务内：
       - prepare_cached("INSERT OR IGNORE INTO photos(id,path,status,created_at) VALUES(?,?,'pending',?)")
       - prepare_cached("SELECT status, created_at FROM photos WHERE id=?")
       - for p in matched:
           id  = blake3(path_str.as_bytes()).to_hex()
           now = now_iso8601()
           INSERT OR IGNORE (id, path, now)         // 已存在 path → 靠 UNIQUE 跳过
           (status, created_at) = SELECT by id      // 取真实状态（已存在的可能是 keep/reject）
           push PhotoRow{ id, path, status, created_at }
       - commit
  3. 返回 matched 对应的 PhotoRow（顺序 = walkdir 顺序）
```

设计取舍：
- **id = blake3(path 字符串)**（ARCHITECTURE 锁定）。非内容哈希 → 同图不同路径是两条（PRD 已声明 M1 不做内容级去重）。
- **增量去重靠 `path` UNIQUE + INSERT OR IGNORE**：已存在的不覆盖 status/created_at；"不重复哈希"对路径哈希成本极低，OR IGNORE 已满足语义。
- **回查真实状态**：重导入时已被标 keep/reject 的照片返回真实 status，前端不会误显示成 pending。
- 全程一个 `spawn_blocking`（见 §4），大目录不卡 UI。**不用 rayon**：导入是 I/O 受限的目录遍历 + 廉价路径哈希，rayon 留给 analysis-subsystem 的算力密集分析。

### 已知边界（MVP 接受）
- 非 UTF-8 路径：`to_string_lossy` 可能产生不可逆字符串。M1 假设路径为 UTF-8；极少见，记为已知限制。
- 目录扫描期间文件被外部删除：walkdir 读到的条目仍尝试插入，不校验文件可读性（import 只看扩展名，不打开文件）。

## 4. 并发与连接持有

- `AppState._db: Mutex<Connection>`（未用）→ 改 **`db: Arc<Mutex<Connection>>`**（仍是 `tokio::sync::Mutex`，符合 quality-guidelines「tokio::sync::Mutex<Connection> is fine」）。
- command 内模式（满足 database-guidelines「不得跨 .await 持有 Connection」）：
  ```rust
  let db = state.db.clone();                 // 克隆 Arc，立即释放外层引用
  let rows = tauri::async_runtime::spawn_blocking(move || {
      let conn = db.blocking_lock();          // 阻塞线程内 blocking_lock，不在异步上下文 await
      scanner::scan_folder(&conn, &root)
  }).await.map_err(|e| AppError::Io(e.to_string()))??;
  ```
- 单连接 + 串行化对 MVP 足够；连接池留到 analysis 批处理再议。

## 5. 错误处理

- 延续 M0 手写 `AppError` enum（**不**解决 error-handling.md 的 thiserror/anyhow OPEN）。新增变体：
  ```rust
  #[serde(tag = "kind", content = "message")]
  pub enum AppError {
      Sidecar(String),
      Db(String),
      Io(String),
      NotFound(String),   // path 不存在 / 不是目录
  }
  ```
- `scanner` 内层继续用 `Result<T, Box<dyn Error + Send + Sync>>` 占位，到 `commands/photos.rs` 边界 `map_err` 成 `AppError`。
- command 先校验 `root.is_dir()`，否则 `AppError::NotFound`。
- 前端 `types/ipc.ts` 的 `AppErrorPayload` 联合同步加 `Db|Io|NotFound`；`App.tsx` 按 `kind` 给用户提示文案（i18n 在前端）。

## 6. 前端视图与遗留 echo

- `App.tsx` 主视图改为导入流：`[导入文件夹]` 按钮 → `pickFolder()` → 取消则 no-op；选中 → `scanFolder(path)` → `photosStore.addPhotos` → 显示「已导入 N 张」+ 文件名列表（`busy` 态防重复点）。
- **echo**：`echo_via_sidecar` command 与 `api/echo.ts` 保留（sidecar 健康探针，analyzers 落地前仍有用），但从 App 主视图移除按钮。不删 Rust command（已注册 API 非死码）。
- 列表暂不虚拟化（TanStack Virtual 归 group-browse-ui）；import-scan 只验证管线，列表用普通渲染，量大时可后续替换。
- Zustand `photosStore`：本任务首个 store。store 分区（mega/per-domain/slice）仍 OPEN，本任务按 per-domain 起一个 `photosStore`，标注后续可能重构为 slice，不视为锁定。

## 7. 依赖与权限

- **Cargo**：`walkdir = "2"`、`blake3 = "1"`、`time = { version = "0.3", features = ["formatting"] }`（`OffsetDateTime::now_utc().format(Rfc3339)` 产 ISO8601）、`tauri-plugin-dialog = "2"`。
- **npm**：`zustand`、`@tauri-apps/plugin-dialog`。
- **lib.rs**：`.plugin(tauri_plugin_dialog::init())`。
- **capabilities/default.json**：`permissions` 增 `"dialog:allow-open"`（仅 open，不放 save）。
- ⚠️ **更正（评审 #4）**：CSP 写了 `asset:` 但这 **不等于** 启用资产协议。Tauri v2 需 `app.security.assetProtocol.enable = true` + `scope` 才会注册协议处理器。本任务不渲染图片故不触发，但 `convertFileSrc` 生成的 `src` 当前是 dead URL。启用职责与 scope 设计已回填到 ab-compare 的 D5（谁先渲染图片谁负责）。

## 8. 测试

- `scanner/mod.rs` `#[cfg(test)] mod tests`（dev-dep `tempfile`）：
  - 建临时目录含 `a.jpg / b.PNG / c.heic / note.txt / sub/d.jpeg`，用 `Connection::open_in_memory()` 跑 0001 migration。
  - 断言：仅 4 张图入库，txt 被过滤；子目录递归到。
  - 二次扫描同目录：行数不变（增量幂等）。
  - id 为 64 hex 且对同一路径稳定。
  - 预置一行 `status='keep'` 后重扫：该行返回的 status 仍是 `keep`（回查正确）。
- 不起真实 sidecar（本任务不涉及）。

## 9. 验收映射（见 prd.md Acceptance）

| PRD 验收 | 由谁保证 |
| --- | --- |
| 混合格式只入图片、HEIC 在内 | scanner 扩展名过滤 + 测试 |
| 重复导入不增行/不重哈希 | INSERT OR IGNORE + 幂等测试 |
| id=blake3 hex / status=pending / created_at ISO8601 | scanner 写入 + 测试 |
| 大目录不卡 UI | spawn_blocking |
| 三层 formatter | rustfmt / prettier；本任务不动 python |

## 10. 不做（明确）

- 任何分析、sidecar 调用、分析列 / migration。
- 缩略图、图片渲染、虚拟列表、进度事件、内容级去重、连接池、rayon。

## 11. 已知局限 / 后续待办（评审遗留，本任务接受）

代码评审（max effort）确认、但判定超出 import-scan MVP 范围、留待后续处理的项：

- **#6 路径拼写差异致重复**：`id = blake3(路径串)`。同一物理文件经大小写不同 / UNC / `\\?\` 长路径 / 末尾分隔符等不同写法重导入，会因路径串不同而生成两行。属 design §3 已声明的「同图不同路径视为两条」的延伸；彻底解决需路径规范化（`canonicalize` 在 Windows 产 `\\?\` 前缀，牵连 `convertFileSrc`，是独立取舍）。**留待专门的去重/reconcile 任务。**
- **#7 无 reconcile**：磁盘上删除/移动的文件不会从 `photos` / store 中清除，计数会含已不存在的文件。incremental import 只增不减是当前设计；reconcile/sync 属后续能力（参考 M2 思路）。**记入 backlog。**
- **#8 大目录单事务 + 全程锁**：超大目录（数万张）会以单事务 + 全程持锁运行，阻塞其它 DB 命令、WAL 膨胀。PRD 目标为数百张，MVP 规模内可接受；超大规模需 chunked commit / 批量提交。**记入 backlog。**
