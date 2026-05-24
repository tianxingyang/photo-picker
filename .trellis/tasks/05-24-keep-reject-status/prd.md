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
- 批量状态变更的事务边界（用于多选手动改状态，非自动联动）。

## 软依赖

完全独立。`photos.status` 列 M0 已建，直接做即可，不依赖 similar-grouping。

## Acceptance Criteria

- [ ] 调 `set_status` 后 `photos.status` 落库，重启应用后状态保持。
- [ ] 非法状态值被拒（DB CHECK + command 校验）。
- [ ] 设某张 `keep` 时，同组其余照片状态**不变**（验证无自动联动）。
- [ ] 前端乐观更新，command 失败时回滚到原状态。
- [ ] 三层 formatter 通过。
