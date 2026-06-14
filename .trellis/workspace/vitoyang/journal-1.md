# Journal - vitoyang (Part 1)

> AI development session journal
> Started: 2026-05-22

---



## Session 1: M0 code-review fixes: sidecar concurrency, error display, build chain

**Date**: 2026-05-24
**Task**: M0 code-review fixes: sidecar concurrency, error display, build chain
**Branch**: `main`

### Summary

Ran max-effort code review on M0 scaffold (a35d8a3) and surfaced 8 findings; user added an off-list scaffold bug (@types/node missing) during verification. Codex rescue agent hung on PowerShell sandbox declines and produced nothing usable, so fixes were applied direct-from-source. All 9 fixes land in one commit (a785d4e) with cargo check/clippy + tsc -b + vite build + ruff + python smoke-test green. Two anti-patterns sunk into spec: outer-Mutex-across-await for shared async resources (backend/quality-guidelines.md) and String(e) on Tauri rejections (frontend/quality-guidelines.md).

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `a785d4e` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 2: M1 分析子系统 code-review 修复

**Date**: 2026-05-25
**Task**: M1 分析子系统 code-review 修复
**Branch**: `feat/m1-analysis-subsystem`

### Summary

对 M1 分析子系统做 max 强度 code-review,定位 13 项发现并用 agent team(Python/Rust 并行)逐项修复:EXIF 子 IFD 读取、日期校验、先转灰再缩、曝光边界/护栏、main.py 安全序列化;sidecar::call 双层 Result 区分传输/单文件错误、analyze_one 传输错误保留 pending 可重试、analyze_pending 部分汇总不丢进度、AtomicBool 单飞护栏。trellis-check 全 PASS(pytest 14 / cargo test 11 / clippy/fmt/ruff 干净)。契约沉淀进 error-handling.md 与新建 analyzer-guidelines.md。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `94d7b7d` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 3: M1 近重复分组 (pHash 连通分量)

**Date**: 2026-05-26
**Task**: M1 近重复分组 (pHash 连通分量)
**Branch**: `main`

### Summary

规划并实现 similar-grouping 子任务：数据模型从一照一组改为多方法多对多落库 (similar_groups + group_members)，照片可跨方法属多组，M3 零迁移复用。算法定为纯 pHash + 连通分量/单链 (弃用时间窗)，阈值 8 存 params 可调，孤立不分组，组 id 由 blake3(method+排序成员) 派生保证重跑幂等。新增 migration 0003、grouping/mod.rs 纯逻辑、commands/grouping.rs::group_photos (single-flight, spawn_blocking)。26 测试绿，PR #3 squash 合并 main 后归档。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `4bf7bcc` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete

---

## Backlog (analysis subsystem — NOT group-browse-ui)

**Date**: 2026-05-26
**Source**: surfaced while verifying group-browse-ui on real photos

- **Blur metric false-positives on shallow depth-of-field (bokeh).** `python/analyzers/blur.py` uses whole-image Laplacian variance + a single global threshold. A large-aperture shot with a sharp subject and an intentionally blurred background has low *global* variance (the blurred area dominates) and is wrongly flagged `is_blurry=true` — often the keeper shots. Fix direction (preferred): tile the image, take a high percentile / max of per-tile Laplacian variance ("sharp anywhere => not blurry"), then re-calibrate the threshold + update analyzer tests. Alt: center/saliency-weighted subject sharpness, or defer to M3 face/saliency. Needs its own task. UI only renders `is_blurry`, so no group-browse change.
- **`analyze_pending` does not retry `failed` rows.** Only `analysis_state='pending'` is reprocessed; rows that failed (e.g. the pre-fix UTF-8 path bug) stay `failed` until manually reset. Consider a "re-analyze failed" action.


## Session 4: M1 group-browse UI + sidecar UTF-8 fix

**Date**: 2026-05-26
**Task**: M1 group-browse UI + sidecar UTF-8 fix
**Branch**: `feat/m1-group-browse-ui`

### Summary

Planned + built the similar-group browse UI (main review surface). Backend: list_groups query command returning groups (within-group sorted by shot_at asc, blur desc) + an ungrouped bucket, camelCase DTO, spawn_blocking read, registered in lib.rs, 5 unit tests. Enabled Tauri assetProtocol (+ protocol-asset feature) so convertFileSrc asset:// URLs load — verified images render on Windows. Frontend: introduced Tailwind v3 + shadcn-style tokens (resolves the project styling OPEN decision), per-domain groupsStore with optimistic setStatus, groupsApi/analysisApi boundary validation, and a TanStack-Virtual flattened-row grid with PhotoCard (status pill + quality badges + A/B & status mount points). Moved dev port 1420->5173 (Windows reserved range). Separately fixed an analysis-subsystem bug: Python sidecar stdin/stdout forced to UTF-8 so CJK/OneDrive paths analyze (cp936 mis-decode caused FileNotFoundError). Backfilled the styling + store-partitioning spec decisions. Backlogged (journal): whole-image blur metric false-flags shallow-DoF/bokeh; analyze_pending doesn't retry failed rows. GUI grouping render still pending user visual confirmation. trellis-check passed; all automated gates green.

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `be010f9` | (see git log) |
| `f346945` | (see git log) |
| `875e0db` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 5: M1 ⑨ keep/reject/pending status persistence (set_status)

**Date**: 2026-05-26
**Task**: M1 ⑨ keep/reject/pending status persistence (set_status)
**Branch**: `feat/m1-keep-reject-status`

### Summary

Planned+implemented the photo three-state status loop: frontend optimistic setStatus -> Rust set_status command (enum-check->Validation, spawn_blocking single-row UPDATE, 0 rows->NotFound) -> DB, no new migration (reused 0001 status enum+CHECK). Locked 4 decisions: single-photo only (no batch - no multi-select UI in M1), AppError::Validation variant (paired error.rs<->ipc.ts), missing id->NotFound, silent optimistic rollback. trellis-check passed 7/7 AC + 36 Rust tests; GUI smoke (persist across restart, no group-linkage) user-confirmed. A parallel session (e7578aa) fixed a stale-rollback race with per-id single-flight + persisted-baseline rollback; captured that pattern + the AppError::Validation contract + tsc -b/vite-env.d.ts gotchas into specs.

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `3a19008` | (see git log) |
| `e7578aa` | (see git log) |
| `89d6e14` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 6: A/B compare 代码审查 + 修复 HEIC 切图 stale 帧

**Date**: 2026-05-27
**Task**: A/B compare 代码审查 + 修复 HEIC 切图 stale 帧
**Branch**: `feat/ab-compare`

### Summary

对 feat/ab-compare 分支做 max 级代码审查（5 角度 finder + 交叉验证 + sweep）。确认契约层全通过（命令名/参数大小写 photoId->photo_id/payload 键/返回 shape/assetProtocol scope=["**"]/CSP 含 asset:），排除两个误报（overlay z-50 trap focus 期间 load() 不可达，故 memberIds 不会 stale；sidecar 串行处理故 .part 无并发覆盖）。修复 1 号发现：useDisplaySrc hook 实例切图复用 + useState 初始化器仅首挂载运行 + loading 重置在 paint 后 effect -> HEIC<->HEIC 导航/Swap 首帧把旧图画到新 id。改为渲染期重置（prevIdRef 守卫 + plain-value setHeicState，不触碰 ef0f1aa ref-in-updater 陷阱），React commit 前丢弃 stale 渲染。tsc -b 通过，trellis-check PASS。未处理的低/中危发现待后续：#3 滚轮缩放泄漏给 webview（React onWheel passive 无法 preventDefault，需 ref 挂 non-passive 监听）、#4 transcode.py 裸文件名 dest 时 os.makedirs('') 崩（当前 Rust 调用方传绝对路径故不可达）、#5 工具栏 aPhoto 用非响应式 getState 快照、#6 mtime 缓存键在 FAT/exFAT 2s 粒度上可能失效。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `77b59bc` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 7: 导出精选 PR#8 审查修复（失败明细 + 源在目标内跳过）

**Date**: 2026-06-08
**Task**: 导出精选 PR#8 审查修复（失败明细 + 源在目标内跳过）
**Branch**: `feat/m1-export-selection`

### Summary

对 PR #8 (export_keep) 做 max-effort 多 agent 审查，修复两项发现：(1) 前端不再丢弃 failed[].{source,reason}，新增可折叠 <details> 明细 + aria-live 失败计数；(2) ExportSummary 新增 skipped，copy_keeps 规范化源文件，父目录==目标时跳过，避免把 keep 克隆回源库。对抗复核抓到并修掉自引入回归：跳过判断改为规范化源文件本身，已删除的 keep 仍落入 failed。新增 3 个 Rust 测试（55/55 通过），tsc 通过，trellis-check 通过（自修 fmt/prettier）。ARCHITECTURE.md 契约同步。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `b84feba` | (see git log) |
| `3cff828` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 8: Milestone 1 MVP 最终集成验收与父任务收尾

**Date**: 2026-06-13
**Task**: Milestone 1 MVP 最终集成验收与父任务收尾
**Branch**: `chore/m1-mvp-integration-wrapup`

### Summary

M1 parent 集成验收：自动化全绿（Rust 61 tests / Python 25 tests / tsc+vite build / clippy -D warnings / ruff；DB 只读核查迁移 v3、328 张导入、126 张分析列齐全、20 个 phash_burst 组、status 持久化），用户 GUI 实测端到端链路（含 HEIC 混合导入、A/B 键盘 1/2、重启状态保持、导出 keep-only）通过。回填 parent PRD 验收清单、ROADMAP M1 标记交付并回填决策池，归档 05-24-milestone-1-mvp，M1 收官。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `d3e9a64` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 9: 项目级工作区隔离 (project-isolation)

**Date**: 2026-06-14
**Task**: Project-based workspace isolation
**Branch**: `feat/project-isolation` (PR #10, merged) → `chore/finish-project-isolation` (wrap-up)

### Summary

单 DB 改为项目级隔离会话。迁移 0004 新增 `projects` 表（UUID v4 主键），drop+重建 `photos`/`similar_groups`/`group_members` 并加 `project_id` TEXT FK（`ON DELETE CASCADE`）+ `UNIQUE(project_id, path)`；按 R5 丢弃 dev 旧数据。照片 id 改为 `blake3(project_id + "\n" + path)`，同路径在两项目独立成两条记录。`AppState.current_project: Mutex<Option<String>>` + `current_project()` 守卫贯穿 `scan`/`export`/`analyze`/`group`/`list_groups`；`set_status`/`transcode` 走全局唯一 id 不改。新增 `create/list/open/close/delete_project` 命令。前端落地页路由（名称+照片数+最后打开、新建、删除二次确认），打开/关闭项目清空 photo/group/compare store + display cache。后端 70 测试 / clippy / fmt / tsc / vite build 全绿。对抗复核 16/16 spec 断言 supported，契约沉淀进 `backend/database-guidelines.md` + `error-handling.md`。

### Main Changes

- backend: migration 0004 + scanner id 派生 + AppState 守卫 + projects 命令 + 5 命令作用域化。
- frontend: projectsApi / projectsStore / LandingView + App.tsx 路由。
- spec: project-isolation 契约（schema / id / 作用域 / Validation 守卫）。

### Git Commits

| Hash | Message |
|------|---------|
| `b79f5be` | feat(m2): project-based workspace isolation (#10) |
| `db5e376` | docs(spec): capture project-isolation backend contracts |

### Testing

- [OK] cargo test 70 passed; clippy -D warnings clean; cargo fmt --check clean.
- [OK] tsc --noEmit clean; vite build pass; new frontend files eslint clean.
- [OK] spec adversarial verify 16/16 supported (file:line evidence).
- [PENDING] GUI 冒烟由用户驱动（多项目隔离 + 删除级联）。

### Status

[OK] **Completed**

### Next Steps

- 整图模糊把浅景深误判为模糊（bokeh backlog），后续独立任务处理。
