# Design — 导出精选

> Parent: `05-24-milestone-1-mvp`｜功能 ⑩。读 prd.md「已决策」获取冲突策略锁定取值（=改名加序号）。

## 1. 边界与既有接缝

本任务是 M1 最后一块：把 `status='keep'` 的原片**复制**到用户选的目录。沿用前序子任务铺好的所有结构，**不新建迁移、不新增 AppError 变体、不改 IPC 类型契约**：

| 已存在 | 位置 | 本任务动作 |
| --- | --- | --- |
| `photos(id, path, status, …)` 表 + status 索引 | `db/mod.rs` 迁移 | **只读查询**，无 schema 变更 |
| `pickFolder()`（原生目录选择器，返回绝对路径或 null） | `src/api/dialogApi.ts` | **复用**，导出目标目录直接调它 |
| `tauri_plugin_dialog` | `lib.rs:36` 已注册 | 复用，无需加插件 |
| 命令模式 `state.db.clone()`+`spawn_blocking`+`blocking_lock`+纯函数 | `commands/photos.rs` `set_status` | 照搬到 `export_keep` |
| `AppError {Sidecar,Db,Io,NotFound,Validation}` | `error.rs` | **复用 `Io`/`Validation`/`Db`**，不新增变体 |
| `AppErrorPayload`/`KINDS`（含全部 5 kind） | `src/types/ipc.ts` | **不动**（`Io`/`Validation` 已存在） |
| `notice`/`error` 结果展示 + `describeAppError` | `src/App.tsx` | 加「导出精选」按钮 + 结果文案 |

> **本任务无新增复杂可见 UI**：仅 header 加一个按钮 + 复用现有 notice 文案带。按全局记忆，可见 UI 需走 `ui-ux-pro-max`——这里只是与现有三个 header 按钮同款的一个 `<button className={BTN}>`，沿用既定样式 token，不属于新设计；若实现时要调整按钮区布局/视觉，再触发该 skill。

## 2. Rust 契约（`commands/photos.rs` 新增）

```rust
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub exported: usize,                // 成功落地的张数（含改名落地）
    pub renamed: usize,                 // 其中因目标重名而改成 name (n).ext 的张数
    pub failed: Vec<ExportFailure>,     // 单项失败，不中断整体
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFailure {
    pub source: String,                 // 失败源文件绝对路径
    pub reason: String,                 // 人读原因（io::Error 文案）
}

/// 把 status='keep' 的原片复制到 dest_dir，保留原名；目标重名→ name (n).ext。
/// 源只读、不移动、不改字节。单项失败计入 summary.failed，不中断整体。
#[tauri::command]
pub async fn export_keep(
    dest_dir: String,
    state: State<'_, AppState>,
) -> Result<ExportSummary, AppError> {
    let dest = PathBuf::from(&dest_dir);
    // 目标必须是已存在目录（pickFolder 选的就是已存在目录；防御性校验）。
    if !dest.is_dir() {
        return Err(AppError::Validation(format!("not a directory: {dest_dir}")));
    }

    // 1) 读 keep 路径列表（DB，spawn_blocking 内）。
    let db = state.db.clone();
    let paths = tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<Vec<String>> {
        let conn = db.blocking_lock();
        select_keep_paths(&conn)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?;

    // 2) 复制（纯文件 IO，无 DB；同样放进 spawn_blocking 避免阻塞 tokio worker）。
    let summary = tauri::async_runtime::spawn_blocking(move || copy_keeps(&paths, &dest))
        .await
        .map_err(|e| AppError::Io(e.to_string()))?;

    Ok(summary)
}

/// 纯 DB helper：取 keep 的源路径。
fn select_keep_paths(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("SELECT path FROM photos WHERE status = 'keep'")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// 纯文件 helper：逐个复制到 dest，重名改 name (n).ext。源只读。
fn copy_keeps(paths: &[String], dest: &Path) -> ExportSummary {
    let mut summary = ExportSummary { exported: 0, renamed: 0, failed: Vec::new() };
    for src in paths {
        let src_path = Path::new(src);
        let file_name = match src_path.file_name() {
            Some(n) => n,
            None => {
                // 源路径无文件名 → 计入 failed，绝不静默丢张（守 prd「不静默丢文件」）。
                summary.failed.push(ExportFailure {
                    source: src.clone(),
                    reason: "source path has no file name".into(),
                });
                continue;
            }
        };
        let (target, renamed) = match resolve_target(dest, file_name) {
            Some(t) => t,
            None => {
                // 9999 个候选名都被占 → 计入 failed，不挂死、不丢张。
                summary.failed.push(ExportFailure {
                    source: src.clone(),
                    reason: "too many name collisions (>9999) at destination".into(),
                });
                continue;
            }
        };
        match std::fs::copy(src_path, &target) {
            Ok(_) => { summary.exported += 1; if renamed { summary.renamed += 1; } }
            Err(e) => summary.failed.push(ExportFailure {
                source: src.clone(), reason: e.to_string(),
            }),
        }
    }
    summary
}

/// 在 dest 下为 file_name 找一个不冲突的目标路径。
/// 不存在 → 原名；已存在 → "stem (1).ext"、"stem (2).ext" …探到空位。
/// 返回 (目标路径, 是否改名)。
fn resolve_target(dest: &Path, file_name: &OsStr) -> Option<(PathBuf, bool)> { /* 见下 */ }
```

- **返回 `ExportSummary`**（不是 `()`）：前端需展示「导出 N 张 / M 张改名 / K 项失败」。
- **两段 spawn_blocking**：先 DB 查路径（持锁最短），释放后再做纯文件 IO（不持 DB 锁，避免长复制期间阻塞其它 RPC）。符合 backend 规范「DB 工作进 spawn_blocking、不跨 .await 持锁」。
- **单项失败不中断**：`std::fs::copy` 出错（权限/磁盘满/源消失）→ 推 `failed`，循环继续。满足「单项报错并计入汇总」。
- **0 张 keep**：`paths` 空 → 循环不进 → `exported=0, failed=[]`，正常返回（不是错误）。前端据 `exported==0` 给提示。
- **注册**：`lib.rs` 的 `invoke_handler![...]` 追加 `commands::photos::export_keep`。

### `resolve_target` 改名算法（冲突策略=改名加序号）

```
stem, ext = split(file_name)            // "IMG_001", "jpg"（无扩展名则 ext 为空）
candidate = dest/file_name
if !candidate.exists():  return Some((candidate, false))
for n in 1..=9999 :                        // 有界探测，避免病态目录下无限探盘
    candidate = dest / format!("{stem} ({n}){ext_with_dot}")
    if !candidate.exists():  return Some((candidate, true))
return None                                 // 9999 个候选都被占 → 交回 copy_keeps 计入 failed
```

- **探测磁盘 `exists()`**：本轮内先写的文件已在磁盘上，故跨源同名的第二张自然探到 `(1)`——两张都落地（满足验收）。
- **上限兜底（已定）**：探测 `1..=9999` 仍全被占 → `resolve_target` 返回 `None`，`copy_keeps` 把该源推入 `failed`（reason: too many name collisions），既不挂死也不丢张。签名由 `(PathBuf, bool)` 改为 `Option<(PathBuf, bool)>` 以承载这条错误通道。
- **无扩展名文件**：`stem (1)`（无点）。`.gitignore` 这类隐藏名（全是扩展名）按 OS 规则 `file_stem`/`extension` 解析，边界用例进单测。
- **TOCTOU**：`exists()` 检查与 `copy` 写之间存在竞态（理论上外部进程可能在间隙抢占文件名）。M1 单用户桌面场景忽略；`std::fs::copy` 用的是覆盖语义，但我们只在「探到不存在」时写，正常路径不会覆盖。**已知局限，记 design 不做 O_EXCL**（M1 桌面单用户可接受）。

## 3. 前端契约

### `src/api/photosApi.ts`（新增导出函数 + 结果类型）

```ts
export type ExportFailure = { source: string; reason: string };
export type ExportSummary = { exported: number; renamed: number; failed: ExportFailure[] };

// Tauri v2: JS camelCase `destDir` ↔ Rust snake_case `dest_dir`。
// 在边界做防御性 narrowing（同 scanFolder：Rust↔TS 尚无 codegen）。
function isExportFailure(x: unknown): x is ExportFailure {
  return (
    !!x && typeof x === "object" &&
    typeof (x as ExportFailure).source === "string" &&
    typeof (x as ExportFailure).reason === "string"
  );
}

export async function exportKeep(destDir: string): Promise<ExportSummary> {
  const raw = await invoke<unknown>("export_keep", { destDir });
  if (!raw || typeof raw !== "object") {
    throw new Error("export_keep returned a non-object result");
  }
  const o = raw as { exported?: unknown; renamed?: unknown; failed?: unknown };
  if (
    typeof o.exported !== "number" ||
    typeof o.renamed !== "number" ||
    !Array.isArray(o.failed) ||
    !o.failed.every(isExportFailure)   // 同 scanFolder：连 failed[] 元素形状一起校验
  ) {
    throw new Error("export_keep result shape mismatch");
  }
  return raw as ExportSummary; // 经守卫后收窄
}
```

> 与既有约定一致：`@tauri-apps/api` 仅在 `api/` 出现；组件/store 经 `photosApi` 调用，不直接 `invoke`。

### `src/App.tsx`（加按钮 + 结果文案）

```tsx
async function onExport() {
  if (busy) return;
  setError(null); setNotice(null); setBusy(true);
  try {
    const destDir = await pickFolder();
    if (destDir === null) return;                  // 用户取消
    const { exported, renamed, failed } = await exportKeep(destDir);
    if (exported === 0 && failed.length === 0) {
      setNotice("没有标记为「保留」的照片，未导出任何文件");
    } else {
      const parts = [`已导出 ${exported} 张`];
      if (renamed > 0) parts.push(`${renamed} 张因重名改名`);
      if (failed.length > 0) parts.push(`${failed.length} 项失败`);
      setNotice(parts.join("，"));
    }
  } catch (e) {
    const { message } = describeAppError(e);
    setError(`导出失败：${message}`);
  } finally {
    setBusy(false);
  }
}
// header 加（与现有按钮同款，含 type="button"）：
// <button type="button" onClick={onExport} disabled={busy} className={BTN}>导出精选</button>
// 并在 App.tsx 顶部 photosApi 的 import 里加上 exportKeep。
```

- 沿用 `busy`/`error`/`notice` 三态与 `describeAppError`，与 `onImport`/`onAnalyzeAndGroup` 同构。
- `pickFolder() === null`（取消）→ 静默返回，不报错。

## 4. 数据流

```
用户点击「导出精选」
  └─> Frontend: pickFolder() 拿目标目录（取消→null→静默返回）
       └─> exportKeep(destDir) → invoke('export_keep', { destDir })
            └─> Rust export_keep:
                 ① dest.is_dir() 校验（否→Validation）
                 ② spawn_blocking #1: SELECT path FROM photos WHERE status='keep'
                 ③ spawn_blocking #2: 逐个 resolve_target(探空位) → std::fs::copy
                      · 成功→exported++（改名→renamed++）
                      · 失败→failed.push（不中断）
                 └─> 返回 ExportSummary
            └─> Frontend 据 exported/renamed/failed 拼 notice 文案
```

## 5. 兼容性 / 回滚

- **无 schema 变更、无新 AppError 变体、无 IPC 类型契约变更** → 前向兼容零风险。
- **`ARCHITECTURE.md:138` 措辞接缝**：现写「用户原片：绝不复制、绝不修改」。该句指的是**应用内部存储**（缩略图/分析不复制原片入库），而本任务的导出是**用户主动发起、复制到用户自选目录、源只读不动**。须把该行澄清为「应用内部绝不复制/移动/改写原片；导出是用户显式发起的只读拷出」，避免后人误读为禁止导出。
- 回滚点：改动集中在 `commands/photos.rs`（+命令体+两个纯函数+测试）、`lib.rs`（注册）、`src/api/photosApi.ts`、`src/App.tsx`、`ARCHITECTURE.md`。无迁移、无数据写入（导出只读源、只写用户目录）→ `git revert` 即整体撤回，不留 DB 痕迹。

## 6. 测试策略

- **Rust 单元测试**（`commands/photos.rs` 的 `#[cfg(test)] mod tests`，复用 photos.rs **自己既有的** `mem_conn()`（只加载 `migrations/0001_initial.sql`）+ `insert_photo()` helper + `tempfile` 建临时目录；**不要**用 grouping.rs 那个加载 3 个迁移的版本）：
  - `select_keep_paths`：插 keep/reject/pending 各若干 → 只返回 keep 的 path。
  - `resolve_target` 无冲突 → `Some((原名, false))`。
  - `resolve_target` 单冲突 → `Some((name (1).ext, true))`；双冲突 → `name (2).ext`。
  - `copy_keeps` 跨源同名：两个不同临时目录下各一个 `a.jpg` → 目标得 `a.jpg` + `a (1).jpg`，`exported=2,renamed=1`。
  - `copy_keeps` 源不存在路径 → 进 `failed`，其余仍导出（不中断）。
  - **`copy_keeps` 源无文件名分支** → 进 `failed`（不静默丢张；守 prd「不静默丢文件」，对应 §2 的 None arm）。
  - **`copy_keeps` 零 keep**（`&[]`）→ `exported=0, renamed=0, failed=[]`，覆盖 AC6。
  - **改名不破坏目标已有文件**：dest 预放一个内容已知的 `a.jpg`，导出另一源的 `a.jpg` → 新张落 `a (1).jpg`，**预存的 `a.jpg` 字节不变**（覆盖 AC3「不覆盖目标已有文件」那一半）。
  - 源字节级一致：复制后比对 `std::fs::read(src)==read(target)`；源文件复制后仍存在且未变。
  - 无扩展名 / 仅扩展名（`.gitignore`）边界改名。
- **前端**：仓库无 vitest 基建（沿前序任务结论）→ 不引入测试框架；正确性靠 `tsc` 类型检查 + Rust 测试 + GUI 冒烟。
- **GUI 冒烟（用户自驱，按全局记忆由用户操作）**：标几张 keep → 点导出 → 选空目录 → 确认 keep 都到、源目录原片仍在且未改；再对同一目标目录导出第二次 → 出现 `(1)` 副本；无 keep 时点导出 → 提示「没有保留照片」。
