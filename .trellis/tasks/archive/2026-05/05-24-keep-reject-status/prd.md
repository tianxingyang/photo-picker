# 保留淘汰待定状态闭环

> Parent: `05-24-milestone-1-mvp`｜覆盖 ROADMAP M1 功能 ⑨保留/淘汰/待定

## Goal

实现照片三态（`keep` / `reject` / `pending`）的变更与持久化：前端发起状态切换 → Rust command 写库 → 前端乐观更新。状态是导出环节的依据。D4 已定为不联动，故只处理单张状态、不做组内自动联动。

## Scope

### In Scope

1. Rust command：`set_status(photo_id, status)`（及可选批量），写 `photos.status`（已存在的 TEXT 枚举列）。
2. 前端 Zustand：乐观更新 + 失败回滚；供 group-browse-ui 与 ab-compare 调用。
3. 持久化校验：状态写库，重启应用后保持。

> D4 已锁定为 **不联动**：设 `keep` 只改当前张，同组其余保持原状态。本任务因此**不实现任何组内自动状态联动**，也不依赖 similar-grouping 的 `group_id`。

### Out of Scope

- 撤销/重做历史（M2）。
- 状态触发 UI（按钮/快捷键）本体在 group-browse-ui / ab-compare；本任务提供 command + store action。
- 导出动作 → export-selection。
- 组内自动状态联动（D4=不联动，明确不做）。

## 决策点

- **D4 组内联动 — 已锁定：不联动** ✅（2026-05-24）。设 `keep` 只改当前张。
- **批量状态变更 — 已锁定：M1 不做** ✅（2026-05-26）。M1 无多选 UI（`CardActions` 逐卡片单张、ab-compare 两两对比），批量命令无调用方。仅交付 `set_status(photo_id, status)`；M2 多选落地时按同样的 `spawn_blocking`+事务模式再加 `set_status_batch`，零返工。
- **非法状态值错误契约 — 已锁定：新增 `AppError::Validation`** ✅（2026-05-26）。command 先校验枚举 → 命中返回 `Validation(String)`；DB `CHECK` 作后备。需同步 `error.rs` 枚举 + `src/types/ipc.ts` 的 `AppErrorPayload` union 与 `KINDS`（spec：前端加 `kind` 分支前必须先加 Rust 变体）。
- **photo_id 不存在 — 已锁定：`NotFound`** ✅（2026-05-26）。UPDATE 影响 0 行 → 返回 `AppError::NotFound`（沿用 `scan_folder` 的「非目录→NotFound」模式，`App.tsx` 已处理该 kind），让陈旧 id 浮现而非静默吞掉。
- **失败反馈 — 已锁定：静默回滚** ✅（2026-05-26）。本地单行 UPDATE 近乎不失败；乐观更新失败时状态标签弹回原值即反馈。store 回滚后 rethrow，`PhotoCard` `.catch()` 吃掉（仅 DEV 下 `console.debug`），不新建 toast/notice 基建。

## 软依赖

完全独立。`photos.status` 列 M0 已建，直接做即可，不依赖 similar-grouping。

## Acceptance Criteria

- [ ] 调 `set_status(photo_id, status)` 后 `photos.status` 落库，重启应用后状态保持。
- [ ] 非法状态值被拒：command 枚举校验返回 `AppError::Validation`；DB `CHECK` 作后备（单元测试覆盖两层）。
- [ ] `photo_id` 不存在（0 行受影响）返回 `AppError::NotFound`。
- [ ] 设某张 `keep` 时，同组其余照片状态**不变**（验证无自动联动）。
- [ ] 前端 `groupsStore.setStatus` 乐观更新，`set_status` 失败时回滚到原状态并 rethrow；`PhotoCard` `.catch()` 不留未捕获 rejection。
- [ ] `set_status` 写入 `src/types/ipc.ts` 的 `AppErrorPayload`（新增 `Validation` kind）+ `ARCHITECTURE.md §IPC`。
- [ ] 三层 formatter / lint 通过：`cargo fmt --check` + `cargo clippy -D warnings` + Rust 测试；`prettier --check` + `tsc` 类型检查（Python 不涉及）。
