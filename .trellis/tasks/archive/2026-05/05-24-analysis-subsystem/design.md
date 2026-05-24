# 分析子系统 — Technical Design

> 依据 `prd.md` 已锁定决策：D1 宽表 / D-op 单 `analyze` op / D-conc 单 sidecar 串行 / D-state 独立 `analysis_state` / D-trigger 显式 `analyze_pending` / D2 轻量栈全局阈值。
> 现有基建参见 `src-tauri/src/{sidecar,db,scanner,commands,error}.rs`、`python/main.py`、`migrations/0001_initial.sql`。

## 1. 边界与职责

```
Frontend (本任务不动)
   │ invoke('analyze_pending')           ← UI 任务接线（扫描后自动 / 按钮）
   ▼
Rust commands/analysis.rs                ← 新增：取 pending 行、串行派发、落库、返回计数
   │ sidecar.call("analyze", {path})     ← 复用现有 Sidecar（单子进程、id 多路、30s 超时）
   ▼
Python analyzers/analyze.py              ← 新增：解码一次 → blur/exposure/phash/exif 四算法 → 合并
   │ 结果 JSON
   ▼
Rust persist_analysis(conn, id, result)  ← 新增：宽表列写回 + analysis_state 流转
   ▼
SQLite photos（migration 0002 加列）
```

模块归属遵守 `directory-structure.md`：`commands/` 只做编排不含算法；`sidecar/` 是 Python 子进程唯一入口（**本任务不改 `sidecar/mod.rs`**，仅调用其 `call`）；`db/` 是 Connection 唯一持有者；Python 一算法一 module。

## 2. IPC 契约（新增 `analyze` op）

请求（Rust → Python），payload 预留阈值覆盖位（MVP 不发，分析器取默认常量）：

```json
{ "id": 42, "op": "analyze", "payload": { "path": "C:/photos/IMG_001.jpg" } }
```

成功响应（camelCase，沿用现有 IPC/`PhotoRow` 约定）：

```json
{ "id": 42, "result": {
  "shotAt": "2026-05-24T10:30:00",      // 无 EXIF → null
  "blurScore": 124.7,
  "isBlurry": false,
  "exposureScore": 0.42,                  // 归一灰度均值 [0,1]
  "exposureFlag": "normal",               // normal | over | under
  "phash": "ffc3a18000000000"             // 64-bit → 16 hex
} }
```

失败（坏文件 / 解码失败）：`{ "id": 42, "error": "UnidentifiedImageError: ..." }`，dispatch 循环不中断（现有 `handle()` 已保证）。

`op` 由 4 枚举（`blur/exposure/phash/exif`）改为单个 `analyze`，`embed`/`face` 预留不变 → 同步改 `ARCHITECTURE.md §IPC`。

## 3. 数据模型（migration `0002_analysis.sql`）

宽表加列，`ALTER TABLE ADD COLUMN`（forward-only，存量行取 DEFAULT）：

```sql
ALTER TABLE photos ADD COLUMN shot_at        TEXT;     -- ISO8601，无 EXIF 为 NULL
ALTER TABLE photos ADD COLUMN blur_score     REAL;
ALTER TABLE photos ADD COLUMN is_blurry      INTEGER CHECK (is_blurry IN (0,1));
ALTER TABLE photos ADD COLUMN exposure_score REAL;
ALTER TABLE photos ADD COLUMN exposure_flag  TEXT CHECK (exposure_flag IN ('normal','over','under'));
ALTER TABLE photos ADD COLUMN phash          TEXT;
ALTER TABLE photos ADD COLUMN analysis_state TEXT NOT NULL DEFAULT 'pending'
                              CHECK (analysis_state IN ('pending','done','failed'));
ALTER TABLE photos ADD COLUMN analysis_error TEXT;     -- failed 时记原始 error 文本

CREATE INDEX IF NOT EXISTS idx_photos_analysis_state ON photos(analysis_state);
CREATE INDEX IF NOT EXISTS idx_photos_shot_at        ON photos(shot_at);
```

注册：在 `db/mod.rs` 的 `MIGRATIONS` 数组追加 `include_str!("../../migrations/0002_analysis.sql")`，version 自动 = 2（现有 `run_migrations` 按下标 +1 驱动 `PRAGMA user_version`）。

要点：
- `status`（`pending|keep|reject`，M0 锁定）**不动**；分析生命周期完全走 `analysis_state`，二者正交。
- SQLite `CHECK` 对 `NULL` 放行，故未分析行的 `is_blurry/exposure_flag` 为 NULL 合法。
- `ADD COLUMN ... NOT NULL` 必须带 DEFAULT（已给 `'pending'`），存量行自动变 pending → 下次 `analyze_pending` 纳入。
- `idx_photos_shot_at` 为 similar-grouping 时间窗口预建（命名遵守 `database-guidelines.md` 示例）。

## 4. Rust 调度（新增 `commands/analysis.rs`）

```rust
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct AnalyzeSummary { pub analyzed: u32, pub failed: u32 }

// 反序列化 sidecar 成功结果；字段 camelCase 对齐 IPC
#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
struct AnalysisResult {
    shot_at: Option<String>,
    blur_score: f64, is_blurry: bool,
    exposure_score: f64, exposure_flag: String,
    phash: String,
}

#[tauri::command]
pub async fn analyze_pending(state: State<'_, AppState>) -> Result<AnalyzeSummary, AppError>
```

流程：
1. 克隆 sidecar `Arc`（在短暂 `lock` 内取出后立即释放，复用 `echo_via_sidecar` 模式；未启动 → `AppError::Sidecar`）。
2. `spawn_blocking` 读 pending 列表：`SELECT id, path FROM photos WHERE analysis_state='pending'`（owned `Vec<(String,String)>`）。
3. **串行**遍历：`sidecar.call("analyze", json!({"path": path})).await`
   - `Ok(v)` → 反序列化 `AnalysisResult` → `persist_analysis(conn, id, Ok(result))`；反序列化失败按 Err 处理。
   - `Err(e)` → `persist_analysis(conn, id, Err(e.to_string()))`。
   - 每张的 DB 写入各走一次 `spawn_blocking`（`db.clone()` → `blocking_lock`），**不跨 `.await` 持有 Connection / 不持有 sidecar 外层锁**（`quality-guidelines.md` 禁项）。增量落库，中途中断不丢已完成进度。
4. 累加 `{analyzed, failed}` 返回。

落库纯函数（便于单测，`commands/analysis.rs` 内或 `db/`）：

```rust
fn persist_analysis(conn: &Connection, id: &str, r: Result<AnalysisResult, String>) -> rusqlite::Result<()>
// Ok  → UPDATE 六列 + analysis_state='done', analysis_error=NULL
// Err → UPDATE analysis_state='failed', analysis_error=<msg>（分析列保持原值/NULL）
```

`prepare_cached` 两条 UPDATE。注册命令到 `lib.rs` 的 `generate_handler!`。

> **D-conc 可替换性**：把「分析一张并落库」收敛为循环体内一个 `analyze_one(&sidecar, &db, id, path)`，将来换 sidecar 进程池 / 并发只动循环，不动 persist 与 schema。MVP 不引入 trait/rayon，避免过度设计。

## 5. Python 分析器

布局（一算法一 module，`analyze.py` 编排一次解码）：

```
python/analyzers/
  analyze.py     # run(payload): 解码一次 → 调四算法 → 合并 dict
  exif.py        # extract_shot_at(pil_image) -> str | None
  blur.py        # score(gray_ndarray) -> (float, bool)
  exposure.py    # score(gray_ndarray) -> (float, str)
  phash.py       # compute(pil_image) -> str
  constants.py   # 阈值常量（集中，payload 可覆盖）
```

`main.py`：`OPS["analyze"] = analyze.run`（保留 `echo` 直到全链路验收后清理）。模块导入时 `pillow_heif.register_heif_opener()` 注册 HEIC。

`analyze.run(payload)`：
1. `img = Image.open(payload["path"])`；`img.load()`（HEIC 经已注册 opener）。
2. `exif.extract_shot_at(img)` → DateTimeOriginal(0x9003) 缺则 DateTime(0x0132)，`"YYYY:MM:DD HH:MM:SS"` → `"YYYY-MM-DDTHH:MM:SS"`；无则 `None`。
3. 归一灰度：`gray = to_gray_ndarray(img, max_side=NORM_MAX_SIDE)`（缩到最长边固定值再转灰度，**保证 blur_score 跨分辨率可比**）。
4. `blur.score(gray)` → 拉普拉斯方差（numpy 3×3 拉普拉斯核卷积，取 `var()`）→ `(blur_score, blur_score < BLUR_VAR_THRESHOLD)`。
5. `exposure.score(gray)` → 归一灰度均值 `mean∈[0,1]` + 高/低光削顶比；`mean>OVER_MEAN 或 高光削顶比>CLIP` → `over`，`mean<UNDER_MEAN 或 低光削顶比>CLIP` → `under`，否则 `normal`。
6. `phash.compute(img)` → `str(imagehash.phash(img))`（hash_size=8 → 64-bit/16hex）。
7. 返回 `{shotAt,blurScore,isBlurry,exposureScore,exposureFlag,phash}`。

异常一律由 `main.handle()` 现有 `try/except` 兜成 `{id,error}`，不杀循环。算法 module 不 catch、不写 stdout（`print()` 会污染 IPC — `quality-guidelines.md` 禁项）；需日志走 stderr。

依赖（`pyproject.toml`，uv 管理）：`pillow`、`pillow-heif`、`numpy`、`imagehash`；dev：`pytest`。`uv lock` 后由 `uv run` 解析（首跑装环境，已被 sidecar 30s 超时覆盖说明）。

## 6. 阈值（`constants.py`，MVP 全局硬编码 + 预留覆盖）

| 常量 | 初值（待 fixture 标定） | 含义 |
| --- | --- | --- |
| `NORM_MAX_SIDE` | 1024 | blur/exposure 归一最长边 |
| `BLUR_VAR_THRESHOLD` | 100.0 | 拉普拉斯方差 < 此值判虚焦 |
| `OVER_MEAN` / `UNDER_MEAN` | 0.80 / 0.20 | 归一灰度均值过/欠曝 |
| `CLIP_RATIO` | 0.50 | 高/低光削顶像素占比阈值 |

初值仅为起点；fixture 测试用**相对断言**（清晰 > 虚焦、白图 over、黑图 under）为主，绝对阈值只对「明显」样本断言，避免脆测。

## 7. 兼容 / 迁移 / 回滚

- migration forward-only（`database-guidelines.md`）；回滚靠删库重建或新写 0003 修正，不写 down。
- 失败可重试：`UPDATE photos SET analysis_state='pending', analysis_error=NULL WHERE analysis_state='failed'` 后重跑 `analyze_pending`。
- 与其他子任务接口：similar-grouping 读 `phash`+`shot_at`；group-browse-ui 读 `blur_score` 排序、`is_blurry`/`exposure_flag` 标记。本任务只生产、不读这些消费方。
- 改 sidecar op 名是破坏性（删 4 枚举语义），但当前仅 `echo` 在用、无消费方依赖 4 枚举，安全。

## 8. 测试策略

- **Python（pytest，`python/tests/`）**：合成 fixture（避免提交版权图、可复现）——
  - 清晰=高频棋盘格 vs 高斯模糊版 → 断 `blur_score` 序 + 明显虚焦 `isBlurry`；
  - 纯白→`over`、纯黑→`under`、中灰→`normal`；
  - `Image` 写入 EXIF DateTimeOriginal → 断 `shotAt` 解析；无 EXIF → `None`；
  - `pillow-heif` 存一张 `.heic` → 断能解码出全字段；
  - 同图两份 `phash` 相等、差异图不等。
- **Rust（`#[cfg(test)]` + in-memory）**：
  - migration：fresh DB 建出新列；v1 旧库（仅跑 0001）升级后存量行 `analysis_state='pending'`。
  - `persist_analysis`：Ok 路径写六列+`done`；Err 路径置 `failed`+`analysis_error`。
  - pending 选择查询：混合 state 下只取 `pending`。
  - 不从 Rust 拉真 sidecar（`quality-guidelines.md`）；`analyze_pending` 编排循环为薄胶水，由 fixture 全链路手动验收覆盖。

## 9. 需同步的 spec / 文档（Phase 3.3）

- `ARCHITECTURE.md §IPC`：op 枚举 4→`analyze` + 新结果形状；§数据流·分析：单 op + `analyze_pending`。
- `error-handling.md`：把与枚举冲突的 `status = analysis_failed` 改为 `analysis_state='failed' + analysis_error`。
- `database-guidelines.md`：Schema Shape OPEN → 标注「M1 analysis-subsystem 锁定宽表」。
- `directory-structure.md`：注明 analyzer 由单 `analyze` op 编排（`analyze.py` 组合四算法 module）。
