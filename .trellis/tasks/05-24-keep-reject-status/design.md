# Design — 保留淘汰待定状态闭环

> Parent: `05-24-milestone-1-mvp`｜功能 ⑨。读 prd.md 的「决策点」获取已锁定取值。

## 1. 边界与既有接缝

前 4 个子任务已为本任务铺好接缝，本任务只「接线」，不新建结构：

| 已存在 | 位置 | 本任务动作 |
| --- | --- | --- |
| `photos.status` TEXT 枚举 + `CHECK` + 索引 | `migrations/0001_initial.sql` | **复用，不新建迁移** |
| `groupsStore.setStatus`（乐观更新已实现 + 持久化 TODO） | `src/store/groupsStore.ts:31-38` | 把 TODO 换成真调用 + 回滚 + rethrow |
| `CardActions` 保留/淘汰/待定按钮（键盘可达） | `src/components/browse/CardActions.tsx` | **不动**（可见 UI 已完成） |
| `PhotoCard.handleStatus`（`void setStatus(...)`） | `src/components/browse/PhotoCard.tsx:23-25` | 改为 `.catch()` 吞掉 rejection（静默回滚） |
| `AppError`（serde `tag="kind" content="message"`） | `src-tauri/src/error.rs` | 新增 `Validation(String)` 变体 |
| `AppErrorPayload` / `KINDS` | `src/types/ipc.ts` | 同步新增 `Validation` kind |
| 命令模式 `spawn_blocking`+`blocking_lock` | `commands/grouping.rs`、`commands/photos.rs` | 照搬到 `set_status` |

> **本任务无新增可见 UI** —— 状态按钮在 group-browse-ui 已交付。故 `ui-ux-pro-max`（项目前端设计规范）在本任务不触发；纯 store/IPC 接线无需走它。ab-compare 才会需要。

## 2. Rust 契约（`commands/photos.rs`）

```rust
const STATUSES: [&str; 3] = ["pending", "keep", "reject"];

/// Set one photo's keep/reject/pending status. Single-row, idempotent UPDATE —
/// no transaction, no single-flight guard (unlike group_photos' delete+reinsert).
#[tauri::command]
pub async fn set_status(
    photo_id: String,
    status: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    // command-level enum check → Validation; DB CHECK is the backstop.
    if !STATUSES.contains(&status.as_str()) {
        return Err(AppError::Validation(format!("invalid status: {status}")));
    }
    let db = state.db.clone();
    let rows = tauri::async_runtime::spawn_blocking(move || -> rusqlite::Result<usize> {
        let conn = db.blocking_lock();
        update_status(&conn, &photo_id, &status)
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?;

    if rows == 0 {
        return Err(AppError::NotFound("no photo with that id".into()));
    }
    Ok(())
}

/// Pure DB helper — runs inside spawn_blocking, returns rows affected.
fn update_status(conn: &Connection, id: &str, status: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE photos SET status = ?2 WHERE id = ?1",
        params![id, status],
    )
}
```

- **返回 `()`**：前端已乐观更新；成功=保留乐观态，失败=回滚。无需回传行。
- **无事务/无 single-flight**：单条 UPDATE 在 SQLite 内原子；不存在 grouping 那种 delete-then-reinsert 竞态。
- **注册**：`lib.rs` 的 `invoke_handler![...]` 追加 `commands::photos::set_status`。

### `error.rs` 变更

```rust
pub enum AppError {
    Sidecar(String),
    Db(String),
    Io(String),
    NotFound(String),
    Validation(String),   // 新增：客户端/入参校验失败
}
// Display 分支补 Self::Validation(m) => write!(f, "Validation: {m}")
```

## 3. 前端契约

### `src/api/photosApi.ts`（新增）

```ts
import { invoke } from "@tauri-apps/api/core";
import type { PhotoId, PhotoStatus } from "../types/photo";

// Tauri v2: JS camelCase `photoId` ↔ Rust snake_case `photo_id`。
export async function setPhotoStatus(id: PhotoId, status: PhotoStatus): Promise<void> {
  await invoke("set_status", { photoId: id, status });
}
```

> 共享于 group-browse-ui 与（将来的）ab-compare —— 两者都经 `photosApi`，不各自 `invoke`（spec：`@tauri-apps/api` 仅在 `api/` 出现）。

### `src/store/groupsStore.ts`（替换 TODO）

```ts
setStatus: async (id, status) => {
  const prev = get().byId[id];
  if (!prev) return;
  set((s) => ({ byId: { ...s.byId, [id]: { ...prev, status } } })); // optimistic
  try {
    await setPhotoStatus(id, status);
  } catch (e) {
    set((s) => ({ byId: { ...s.byId, [id]: prev } }));               // rollback
    throw e;
  }
},
```

### `src/types/ipc.ts`（同步契约）

`AppErrorPayload` union 加 `| { kind: "Validation"; message: string }`，`KINDS` 数组加 `"Validation"`。

### `src/components/browse/PhotoCard.tsx`（处理 rejection）

```ts
const handleStatus = (status: PhotoStatus) => {
  setStatus(id, status).catch((e) => {
    // 静默回滚：状态标签弹回即反馈。失败近乎不触发（本地单行 UPDATE）。
    if (import.meta.env.DEV) console.debug("setStatus failed", e);
  });
};
```

> 满足 spec「async action 两条路径都要处理、不留未捕获 rejection」「不得 `console.log`，`console.debug` 仅 DEV guard」。

## 4. 数据流

```
CardActions 按钮 onClick
  → PhotoCard.handleStatus (.catch 兜底)
    → groupsStore.setStatus  ①乐观写 store（标签即时翻）
      → photosApi.setPhotoStatus → invoke("set_status")
        → Rust set_status: 校验 → spawn_blocking UPDATE → rows==0?NotFound
      → 成功: 保留乐观态  /  失败: 回滚 prev + rethrow → PhotoCard .catch 吞
```

## 5. 兼容性 / 回滚

- **无 schema 变更** → 无迁移、无前向兼容风险。
- 新增 `Validation` 变体是**纯增量**：既有前端 `describeAppError` 对未知 kind 已优雅降级（只取 message），即便漏同步也不崩；但 spec 要求同步，必须补。
- 回滚点：本任务改动集中在 4 个文件 + 1 个新 api 文件，`git revert` 即可整体撤回；无数据迁移需要逆转。

## 6. 测试策略

- **Rust 单元测试**（`commands/photos.rs` 的 `#[cfg(test)] mod tests`，仿 grouping.rs 的 `mem_conn()`，仅需 `0001_initial.sql`）：
  - `update_status` 合法值 → 返回 1，读回确认落库。
  - `update_status` 不存在 id → 返回 0（command 据此映射 NotFound）。
  - DB `CHECK` 后备：直接对非法值 UPDATE → `rusqlite::Error`。
  - `STATUSES.contains` 校验：三个合法值通过、未知值拒。
- **前端**：仓库尚无 vitest 基建（package.json 无 vitest，无 `*.test.ts`）→ 本任务不引入测试框架；前端正确性靠 `tsc` 类型检查 + Rust 测试 + GUI 冒烟。
- **GUI 冒烟（用户自驱）**：点保留/淘汰/待定→标签变；重启应用→状态保持；同组其余张不变。
