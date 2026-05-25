# Implement — 近重复分组 (phash_burst)

## 前置

- 分支：从 main 切 feat 分支（`task.py set-branch`）。
- 阅读 spec：`backend/database-guidelines.md`（migration / 查询约定 / spawn_blocking 硬约束）、`backend/directory-structure.md`、`backend/quality-guidelines.md`。

## 有序清单

1. **migration**：新增 `src-tauri/migrations/0003_grouping.sql`（两表 + 索引，见 design）。在 `src-tauri/src/db/mod.rs` 的 `MIGRATIONS` 数组追加 `include_str!("../../migrations/0003_grouping.sql")`。
   - 验证：db 模块的 fresh / upgrade / idempotent 迁移测试通过；加断言 `user_version == 3` 且 `similar_groups` / `group_members` 可查询。

2. **纯聚类模块**：新增 `src-tauri/src/grouping/mod.rs`（`parse_phash` / `hamming` / `cluster` + union-find）。在 `lib.rs` 加 `mod grouping;`。
   - 单测（seed，无 DB）：
     - ① 两张距离 ≤ 8 → 同组。
     - ② A~B、B~C 距离均 ≤ 8 但 A~C > 8 → 三者传递闭包同组。
     - ③ 距离 > 阈值 → 分开。
     - ④ 孤立照片（无邻居）→ 不出现在任何返回分量。
     - ⑤ `parse_phash` 对非 16-hex / 乱码返回 `None`。
     - ⑥ 打乱输入顺序 → 产出相同分区（确定性 / 幂等基础）。

3. **命令层**：新增 `src-tauri/src/commands/grouping.rs`（`group_photos` + `derive_id` + `GroupSummary`）。在 `commands/mod.rs` 加 `pub mod grouping;`；`lib.rs` invoke_handler 注册 `commands::grouping::group_photos`；`AppState` 加 `grouping_running: AtomicBool`（init `false`）。
   - 单测（in-memory DB，仿 `analysis.rs` 的 `mem_conn`）：
     - 插入若干已知 phash 行 → 调持久化逻辑 → 断言 `similar_groups` / `group_members` 行符合预期分区。
     - 孤立行不产生成员行。
     - 重跑一次 → 断言逐行一致（同 id、同成员）且无残留旧组。
     - `phash IS NULL` 行被 SELECT 过滤掉。

4. **阈值标定**：在真实连拍文件夹跑通后，抽样核对分组数；若误合 / 误拆明显，调 `PHASH_THRESHOLD`（5 / 8 / 10 档），最终值同步到 `params` 与 prd。

## 验证命令

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test            # grouping 单测 + commands::grouping 单测 + db 迁移测试
```

> Windows tool-shell 注意：cargo 需在 PATH 前置（见 auto-memory windows-tool-shell-path）。

## 风险点 / 回滚

- migration 一旦发布即前向；本地开发库可删 `<app_data_dir>/photo-picker.db` 重建。
- 分组为派生数据，回滚 = 删两表行 / 重跑，无损原片与 `photos`。
- **clippy / review 重点**：`spawn_blocking` 内 `blocking_lock`，勿跨 `.await` 持 Connection 锁。
- union-find 注意成员去重与 id 升序，确保 `derive_id` 跨重跑稳定。

## 完成前检查

- [ ] 三层 formatter 通过。
- [ ] AC 全绿（seed + 真实集抽样 + 幂等）。
- [ ] 回填 parent `05-24-milestone-1-mvp/prd.md` 的 D3 行 + 多方法分组模型契约（已在 planning 阶段回填，实现后复核与代码一致）。
- [ ] 视需要更新 `backend/database-guidelines.md`：登记 `similar_groups` / `group_members` 表与多方法分组模型（3.3 spec update）。
