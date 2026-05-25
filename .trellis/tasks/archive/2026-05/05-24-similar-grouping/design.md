# Design — 近重复分组 (phash_burst)

## 边界与定位

纯 Rust 主进程逻辑，**无 Python sidecar**。
- 输入：`photos.phash`（analysis-subsystem 产出，16-hex / 64-bit 字符串，如 `"ffc3a18000000000"`）。
- 输出：`similar_groups` + `group_members` 两表。
- 下游：group-browse-ui 读组渲染（本任务不碰 UI）。

## 数据模型

migration `src-tauri/migrations/0003_grouping.sql`（`user_version` → 3）：

```sql
CREATE TABLE IF NOT EXISTS similar_groups (
  id         TEXT PRIMARY KEY,
  method     TEXT NOT NULL,            -- M1 固定 'phash_burst'
  params     TEXT NOT NULL             -- JSON, e.g. {"threshold":8,"version":1}
);

CREATE TABLE IF NOT EXISTS group_members (
  group_id TEXT NOT NULL REFERENCES similar_groups(id) ON DELETE CASCADE,
  photo_id TEXT NOT NULL REFERENCES photos(id)         ON DELETE CASCADE,
  PRIMARY KEY (group_id, photo_id)
);

CREATE INDEX IF NOT EXISTS idx_group_members_photo   ON group_members(photo_id);
CREATE INDEX IF NOT EXISTS idx_similar_groups_method ON similar_groups(method);
```

- `foreign_keys=ON` 已在 `db::open` 设置；`ON DELETE CASCADE` 保证删组即删成员行。
- junction PK `(group_id, photo_id)` 保证成员唯一。
- 多对多：一张照片可在多个组（跨方法）。M1 只写 `method='phash_burst'`；M3 接 CLIP / face 时新增 method 值 + 各自命令，复用同两表，**无 schema 迁移**。
- 与 parent D1（宽表）一致：分析分数仍在 `photos` 宽行；组成员这种「派生、多对多」关系按 backend spec「heavy/sparse 走独立表」原则独立成表。

### 组 id = 内容派生（保证幂等）

`id = blake3(method + "\n" + sorted(member_photo_ids).join("\n")).to_hex()`

复用 `photos.id` 已用的 `blake3`。同一分区 → 同 id → 同行，重跑逐行一致，直接满足「重跑幂等」AC。

## 算法（纯函数，可独立测试）

模块 `src-tauri/src/grouping/mod.rs`（仿 `scanner/mod.rs` 的「纯逻辑 + 单测」结构）：

- `parse_phash(s: &str) -> Option<u64>`：`u64::from_str_radix(s, 16)`；长度非 16 / 解析失败返回 `None`（调用方跳过）。
- `hamming(a: u64, b: u64) -> u32 = (a ^ b).count_ones()`。
- `cluster(items: &[(String, u64)], threshold: u32) -> Vec<Vec<String>>`：
  - union-find（按秩合并 + 路径压缩）。
  - 对所有 `i < j`，若 `hamming(h_i, h_j) <= threshold` 则 `union(i, j)`。
  - 收集连通分量，**仅返回 size ≥ 2** 的分量；每个分量成员 id 升序、分量间按最小成员 id 排序 → 输出确定（顺序无关）。
  - 复杂度 O(n²) popcount；M1 规模（数百~数千张）毫秒级。

## 命令与数据流

`src-tauri/src/commands/grouping.rs`：

```
group_photos()  [#[tauri::command] async]
  ├─ single-flight：AtomicBool grouping_running + RAII guard（仿 analysis.rs RunGuard）
  └─ spawn_blocking { let conn = db.blocking_lock();
       SELECT id, phash FROM photos WHERE phash IS NOT NULL
       parse_phash → Vec<(id, u64)>（跳过解析失败）
       let comps = cluster(&items, PHASH_THRESHOLD)
       tx = conn.unchecked_transaction():
         DELETE FROM similar_groups WHERE method = 'phash_burst'   -- CASCADE 清成员
         for comp in comps:
           let gid = derive_id("phash_burst", &comp)
           INSERT similar_groups(gid, 'phash_burst', params_json)
           INSERT group_members(gid, photo_id)  -- 逐成员
       tx.commit()
     }
  └─ Ok(GroupSummary { groups, grouped_photos })
```

- `const PHASH_THRESHOLD: u32 = 8;`，写入 `params` JSON（`{"threshold":8,"version":1}`）。
- **不持锁跨 await**：DB 工作全在 `spawn_blocking` 内 `blocking_lock`（backend spec 硬约束，仿 photos/analysis）。
- 触发：前端在 `analyze_pending` 完成后调用，或用户手动「重新分组」。与分析解耦——便于独立测试与重跑幂等。

## 兼容 / 迁移 / 回滚

- 前向 migration，`user_version` 驱动，失败 abort boot（既有机制）。
- 分组是**派生数据**：回滚 = 删两表行 / 重跑即可恢复；原片与 `photos` 行不受影响。
- 多方法位：M3 复用同两表新增 method，零 schema 迁移。

## 权衡

- 纯 pHash 无时间窗 → 可能巧合误合（已与用户确认接受，挑片人工排除）。换得简单 + 不依赖 EXIF。
- 单链连通分量 → 渐变连拍可链式合并；`≤8` 紧阈值限制蔓延；幂等性最佳（连通分量分区唯一）。
- O(n²) → M1 规模可忽略；若 M2/M3 规模上升再上 LSH 多段分桶（候选+校验），不在 M1 范围。
