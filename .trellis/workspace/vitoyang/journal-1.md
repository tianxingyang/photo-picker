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
