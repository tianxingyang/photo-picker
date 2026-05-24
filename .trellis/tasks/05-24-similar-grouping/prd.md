# 近重复分组

> Parent: `05-24-milestone-1-mvp`｜覆盖 ROADMAP M1 功能 ⑥pHash 近重复分组

## Goal

纯 Rust 分组逻辑：读取已算好的 `(id, shot_at, phash)`，先按拍摄时间切窗口，窗口内按 pHash 汉明距离阈值聚类，把近重复连拍归到同一组，结果写入 `similar_groups`，供前端按组展示。

## Scope

### In Scope

1. 新增 migration：`similar_groups` 表（或 `photos.group_id` 列，形状随 design，与 D1 一致）。
2. Rust 取 `(id, shot_at, phash)` 列表（无 sidecar 调用）。
3. 时间窗口切分（如 ±N 秒）；`shot_at` 缺失的处理策略（单独成组 / 按导入顺序兜底）。
4. 窗口内 pHash 汉明距离阈值聚类，分配 `group_id`。
5. 触发时机：分析完成后触发，或前端手动触发（写进 design）。

### Out of Scope

- pHash 的计算（在 analysis-subsystem）。
- 语义相似 / CLIP / DBSCAN（M3）。
- 组内 UI 渲染与排序 → group-browse-ui。

## 决策点

- **D3 时间窗口宽度 + 汉明距离阈值**：本任务标定。窗口仅时间，还是时间+文件名前缀+设备（ARCHITECTURE L126）——M1 建议先「仅时间窗口 + 汉明阈值」。
- 单张不成组：是否允许"组大小=1"，还是只对 ≥2 张建组。

## 软依赖

需 analysis-subsystem 落库的 `phash` + `shot_at`。可用手写 seed 数据（构造已知相似/不相似的 phash + 时间）独立验证聚类正确性，不阻塞。

## Acceptance Criteria

- [ ] migration 升版，分组结构建出。
- [ ] 用构造的 seed 数据：同窗口内汉明距离 < 阈值的归同组，跨窗口或距离过大的分开。
- [ ] `shot_at` 缺失的照片按既定策略处理，不崩。
- [ ] 在真实连拍集上分组数与肉眼判断大致吻合（抽样核对）。
- [ ] 重跑分组幂等（同输入产出同分组，不残留旧组）。
- [ ] 三层 formatter 通过。
