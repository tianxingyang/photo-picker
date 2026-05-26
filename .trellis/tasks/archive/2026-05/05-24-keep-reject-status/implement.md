# Implement — 保留淘汰待定状态闭环

> 执行前读 prd.md（决策点）+ design.md（契约）。改动集中在 6 个文件，无迁移。

## 顺序清单

### Rust（后端先，前端依赖其契约）

- [ ] 1. `src-tauri/src/error.rs`：`AppError` 加 `Validation(String)` 变体 + `Display` 分支 `Self::Validation(m) => write!(f, "Validation: {m}")`。
- [ ] 2. `src-tauri/src/commands/photos.rs`：
  - 加 `use rusqlite::{params, Connection};`（当前文件未引入）。
  - 加 `const STATUSES: [&str; 3] = ["pending", "keep", "reject"];`。
  - 加 `#[tauri::command] pub async fn set_status(...)`（见 design.md §2）：枚举校验→`Validation`；`spawn_blocking`+`blocking_lock`→`update_status`；`rows==0`→`NotFound`。
  - 加纯函数 `fn update_status(conn, id, status) -> rusqlite::Result<usize>`。
  - 加 `#[cfg(test)] mod tests`：仿 `grouping.rs` 的 `mem_conn()`（仅 `0001_initial.sql`）。断言点见 design.md §6。
- [ ] 3. `src-tauri/src/lib.rs`：`invoke_handler![...]` 追加 `commands::photos::set_status`。

### 前端

- [ ] 4. `src/types/ipc.ts`：`AppErrorPayload` 加 `| { kind: "Validation"; message: string }`；`KINDS` 加 `"Validation"`。
- [ ] 5. `src/api/photosApi.ts`：加 `export async function setPhotoStatus(id, status): Promise<void>`（`invoke("set_status", { photoId: id, status })`）。
- [ ] 6. `src/store/groupsStore.ts`：删 `setStatus` 里的 TODO 注释，接入 `setPhotoStatus` + 失败回滚 + rethrow（见 design.md §3）；补 `import { setPhotoStatus } from "../api/photosApi";`。
- [ ] 7. `src/components/browse/PhotoCard.tsx`：`handleStatus` 从 `void setStatus(...)` 改为 `setStatus(...).catch(...)`（DEV-guard `console.debug`）。

### 文档

- [ ] 8. `ARCHITECTURE.md §IPC`：登记 `set_status` op（spec 要求新 IPC op 合并前入档）。

## 校验命令

> Windows tool-shell：`cargo`/`gh` 需先 prepend PATH（见全局记忆）。

```bash
# Rust（在 src-tauri/ 下）
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test                       # 含本任务新增 set_status 测试

# 前端（仓库根）
npx prettier --check "src/**/*.{ts,tsx}"
npx tsc -b --noEmit              # 或 npm run build 的 tsc 阶段
```

- Python 不涉及，跳过 ruff。
- 无 vitest 基建 → 不写前端单测（design.md §6）。

## 风险文件 / 回滚点

- `error.rs` + `types/ipc.ts` 是**成对契约**：两边必须同时改，否则前端 `Validation` kind 走降级分支（不崩但丢分类）。提交前对照两文件。
- `lib.rs` 漏注册 `set_status` → 运行期 `invoke` 报「command not found」，`cargo test` 测不到（测的是纯函数 + command 逻辑，不经 Tauri 注册）→ 必须靠 GUI 冒烟兜底。
- 回滚：6 文件改动 + 无迁移，`git revert <commit>` 整体撤回，无数据需逆转。

## 提交前 review gate

- [ ] `cargo fmt/clippy/test` 全绿，`prettier/tsc` 全绿。
- [ ] `error.rs` ↔ `types/ipc.ts` 的 `Validation` 已成对落地。
- [ ] `set_status` 已在 `lib.rs` 注册、已写入 `ARCHITECTURE.md §IPC`。
- [ ] GUI 冒烟（用户自驱）：点击改状态→落库→重启保持→同组其余张不变。
