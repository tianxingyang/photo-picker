# Implement — 导出精选

> 执行前读 prd.md（已决策：改名加序号）+ design.md（契约）。改动集中在 5 个文件，无迁移、无新 AppError 变体、无 IPC 类型变更。

## 顺序清单

### Rust（后端先，前端依赖其返回契约）

- [ ] 1. `src-tauri/src/commands/photos.rs`：
  - 确认/补 `use` —— `std::path::{Path, PathBuf}`、`std::ffi::OsStr`、`rusqlite::Connection`（已有的不重复）。
  - 加 `ExportSummary` / `ExportFailure`（`#[derive(serde::Serialize)] #[serde(rename_all="camelCase")]`，见 design.md §2）。
  - 加 `#[tauri::command] pub async fn export_keep(dest_dir, state) -> Result<ExportSummary, AppError>`：`dest.is_dir()` 否→`Validation`；spawn_blocking #1 查路径；spawn_blocking #2 复制。
  - 加纯函数 `select_keep_paths(&Connection) -> rusqlite::Result<Vec<String>>`（`prepare_cached("SELECT path FROM photos WHERE status='keep'")`）。
  - 加纯函数 `copy_keeps(&[String], &Path) -> ExportSummary` + `resolve_target(&Path, &OsStr) -> Option<(PathBuf, bool)>`（探空位改名；`1..=9999` 有界，名额耗尽返回 `None`）。`copy_keeps` 对两种情况推 `failed`、**绝不静默丢张**：① 源 `file_name()` 为 `None`；② `resolve_target` 返回 `None`。
  - 加 `#[cfg(test)] mod tests`：复用 photos.rs **自己既有的** `mem_conn()`（只加载 0001）+ `insert_photo()` + `tempfile` 临时目录（**不要**用 grouping.rs 的 3-迁移版）。断言点见 design.md §6（含零 keep、目标已有同名字节不变、源无文件名进 failed 三条新增）。
- [ ] 2. `src-tauri/src/lib.rs`：在 `tauri::generate_handler![...]`（`.invoke_handler(...)` 内，约 lib.rs:63-71）末尾的 `commands::grouping::list_groups` 后**补一个逗号**再追加 `commands::photos::export_keep`（该项当前无尾逗号，直接换行追加会 `expected ','`）。

### 前端

- [ ] 3. `src/api/photosApi.ts`：加 `ExportFailure`/`ExportSummary` 类型 + `export async function exportKeep(destDir): Promise<ExportSummary>`（`invoke("export_keep", { destDir })` + 仿 `scanFolder` 的边界形状校验，**含 `failed[]` 元素逐个 `isExportFailure` 校验**，见 design.md §3）。
- [ ] 4. `src/App.tsx`：加 `onExport`（`pickFolder` → null 静默返回 → `exportKeep` → 据 `exported/renamed/failed` 拼 `notice`；catch→`error`）；header 加「导出精选」`<button type="button" …>`（同 `BTN` 样式，`disabled={busy}`，**与现有两个按钮一样带 `type="button"`**）；并把 `exportKeep` 加进顶部 `photosApi` 的 import。

### 文档

- [ ] 5. `ARCHITECTURE.md`：
  - 加 `## 数据流：导出精选`（仿现有数据流块，见 design.md §4）。
  - **澄清 `:138`** 「用户原片：绝不复制、绝不修改」→ 区分「应用内部绝不复制/移动/改写原片」与「导出为用户显式发起的只读拷出」（见 design.md §5）。

## 校验命令

> Windows tool-shell：`cargo`/`gh` 需先 prepend PATH（见全局记忆 windows-tool-shell-path）。

```bash
# Rust（在 src-tauri/ 下）
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test                       # 含本任务新增 export_keep / resolve_target / copy_keeps 测试
                                 # 若缺 tempfile dev-dep 先 `cargo add --dev tempfile`

# 前端（仓库根）
npx prettier --check "src/**/*.{ts,tsx}"
npx tsc -b --noEmit              # 或 npm run build 的 tsc 阶段
```

- Python 不涉及，跳过 ruff。
- 无 vitest 基建 → 不写前端单测（design.md §6）。

## 风险文件 / 回滚点

- **`copy_keeps` 必须只读源**：用 `std::fs::copy(src, target)`（读源→写新目标），绝不 `rename`/`remove`/写回源路径。单测断言「复制后源仍存在且字节不变」是这条的守门。
- **`resolve_target` 死循环 / 上限（已定）**：探空位用 `for n in 1..=9999` 有界循环；耗尽返回 `None`，由 `copy_keeps` 把该源推入 `failed`（不挂死、不丢张）。签名为 `Option<(PathBuf, bool)>` 以承载这条错误通道——这是「不静默丢文件」的第二道守门（第一道是源无 `file_name` 也推 `failed`）。
- **跨源同名靠 `exists()` 探测**：必须在 *复制写盘后* 才认下一个候选——即逐张 `resolve_target` 紧接 `copy`，不可先批量 resolve 再批量 copy（否则两张同名都解析到原名，第二张覆盖第一张）。顺序在 `copy_keeps` 循环内保证。
- **`lib.rs` 漏注册** → 运行期 `invoke("export_keep")` 报 command not found，`cargo test` 测不到（测纯函数+命令逻辑，不经 Tauri 注册）→ 靠 GUI 冒烟兜底。
- 回滚：5 文件改动、无迁移、无 DB 写入，`git revert <commit>` 整体撤回，无数据需逆转。

## 提交前 review gate

- [ ] `cargo fmt/clippy/test` 全绿，`prettier/tsc` 全绿。
- [ ] `export_keep` 已在 `lib.rs` 注册。
- [ ] `ARCHITECTURE.md` 已加导出数据流 + 澄清 `:138` 措辞。
- [ ] Rust 测试覆盖：保留原名 / 改名加序号 / 跨源同名两张都落地 / 源失败不中断 / 源字节不变 / **零 keep 返回 0** / **目标已有同名文件字节不变** / **源无文件名进 failed（不静默丢张）**。
- [ ] GUI 冒烟（用户自驱）：keep 全导出、源未动、重复导出出 `(1)`、无 keep 给提示。
