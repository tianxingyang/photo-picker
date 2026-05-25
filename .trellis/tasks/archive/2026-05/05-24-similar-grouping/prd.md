# 近重复分组

> Parent: `05-24-milestone-1-mvp`｜覆盖 ROADMAP M1 功能 ⑥pHash 近重复分组

## Goal

纯 Rust 分组逻辑：读取已算好的 pHash，按汉明距离做近重复聚类（**连通分量 / 单链**），把近重复照片归到同一组，结果落库到**多方法分组模型**（`similar_groups` + `group_members`），供前端按组浏览与挑片。

## 数据模型（与 parent 共享契约 · 已锁定 2026-05-25）

M1 锁定「**多方法多对多 + 落库**」分组模型（用户决定）：

- `similar_groups(id, method, params)`：组实体。`method` 标识分组方法，M1 写 `'phash_burst'`；`params` 存算法参数（阈值、版本）的 JSON。不存 `created_at`：组是无状态全量重建的派生缓存、id 由内容派生，per-run 时间戳无正当消费者，故 M1 不设（将来 group-browse-ui 真需要时再带明确语义补列）。
- `group_members(group_id, photo_id)`：成员 junction，**多对多**。一张照片可属多个组（跨方法）；M3 语义 / 人脸分组复用同两表、零迁移接入。
- `phash_burst` 方法内一张照片属至多一组；跨方法可属多组（如 M3 一张含两人照片可同进两个人脸组）。

**为什么落库而非现算**：M1 的 pHash 分组虽是纯函数、现算也便宜，但模型要服务 M3——CLIP / 人脸是昂贵 ML 产物，绝不能每次浏览现算，必须持久化；且「同时持有多套分组对比」也要求按方法分别落库。落库结果视为「可失效的派生缓存」，靠重跑幂等保证一致。

## Scope

### In Scope

1. 新增 migration `0003_grouping.sql`：`similar_groups` + `group_members` 两表 + 索引（`user_version` → 3）。
2. Rust 取 `(id, phash)`（`phash IS NOT NULL`），无 sidecar 调用。
3. 纯 pHash 聚类：16-hex phash 解析为 `u64`，按汉明距离阈值做**连通分量**聚类（不依赖时间）。
4. 仅对 size ≥ 2 的连通分量建组并写 `group_members`；孤立照片不写成员行（未分组）。
5. 暴露 `group_photos` 命令：前端在分析完成后或手动触发；**重跑幂等**（清掉 `method='phash_burst'` 旧组后重算）。

### Out of Scope

- pHash 的计算（在 analysis-subsystem）。
- **时间窗口切分**：本任务明确不用时间轴（用户 2026-05-25 决定）。
- 语义相似 / CLIP / DBSCAN / 人脸分组（M3，复用本模型的多方法位）。
- 组内 UI 渲染与排序 → group-browse-ui。
- 多配置 / 多方法并存对比 UI（M1 只实现 phash_burst 一种方法）。

## 决策（已锁定 2026-05-25）

- **不用时间窗口，纯 pHash 全局聚类**。代价：构图巧合相似的无关照片（两面白墙、两张逆光）可能被并组，挑片时人工排除，可接受。换得不依赖 EXIF、跨时间也能抓重复、实现更简单。
- **聚类方式 = 连通分量 / 单链**：汉明距离 ≤ 阈值相连即同组（含传递闭包 A~B~C）。结果唯一、与遍历顺序无关，天然满足重跑幂等。代价：渐变长链可能链式合并，由紧阈值限制蔓延。
- **汉明阈值默认 8**（64-bit；存入 `params` 可调，真实连拍集标定）。
- **孤立照片不分组**：size-1 连通分量不写成员行，UI 归「未分组」区。
- **触发 = 独立 `group_photos` 命令**，与 analyze 解耦，幂等可重跑。

## 软依赖

需 analysis-subsystem 落库的 `phash`。可用手写 seed 数据（构造已知相似 / 不相似的 phash）独立验证聚类正确性，不阻塞。

## Acceptance Criteria

- [ ] migration 升版到 3，`similar_groups` + `group_members` 建出，FK（`ON DELETE CASCADE`）+ 索引就绪；迁移测试断言 `user_version==3`。
- [ ] 用构造的 seed phash：汉明距离 ≤ 阈值的归同组（含传递闭包 A~B~C），距离过大的分开。
- [ ] 孤立照片（无近重复邻居）不出现在任何 `group_members` 行。
- [ ] `phash IS NULL` / 解析失败的照片被跳过，不崩。
- [ ] 重跑 `group_photos` 幂等：同输入产出同分组（组 id 由成员内容派生，逐行一致），不残留旧 `phash_burst` 组。
- [ ] 真实连拍集上分组数与肉眼判断大致吻合（抽样核对）。
- [ ] 三层 formatter 通过（rustfmt + clippy）。
