# Design — 相似组浏览 UI

> Task: `05-24-group-browse-ui` ｜ Parent: `05-24-milestone-1-mvp`
> 技术设计。需求与验收见 `prd.md`；执行清单见 `implement.md`。

## 1. 范围与边界

本任务交付"按相似组浏览"的主审片视图：后端一个查询命令把组结构 + 组内照片（含分析分数与 status）一次性吐给前端，前端用 Zustand 持有、TanStack Virtual 虚拟化渲染，每张缩略图带状态/废片标记，并暴露"进 A/B 对比"和"切 status"的挂载点。

边界（明确不做，留挂载点）：
- **A/B 对比 viewer 本体** → `ab-compare`。本任务点击缩略图只触发 `onCompare(groupId, photoId)` 回调（M1 可先打开占位/console）。
- **status 落库** → `keep-reject-status`。本任务做 optimistic store 更新 + 调用挂载点；真正的 `set_status` 命令属那个任务。若该命令此时未就绪，store action 先只更新本地状态并留 `// TODO(keep-reject-status): invoke set_status`。
- **缩略图缓存 / HEIC 真实解码** → M2 / `ab-compare`(D5)。本任务 HEIC 走占位瓦片。

## 2. 前置阻塞：启用 Tauri assetProtocol（必须最先做）

`convertFileSrc(path)` 在 Tauri v2 生成 `asset://localhost/<encoded path>` URL。`tauri.conf.json` 的 CSP 已放行 `img-src ... asset: http://asset.localhost`，**但 `app.security.assetProtocol` 缺失**——协议处理器没启用、没 scope 白名单，URL 一律加载失败，网格全空白。

改动 `src-tauri/tauri.conf.json` 的 `app.security`：

```jsonc
"security": {
  "csp": "...",                       // 不动
  "assetProtocol": {
    "enable": true,
    "scope": ["**"]                   // M1 取舍：本地单用户桌面，宽松放行
  }
}
```

- **风险点（实现时必验）**：Windows 下盘符绝对路径（`C:\...`）的 glob 匹配有坑，`["**"]` 不一定命中。实现后第一件事就是导入一张真图确认能显示；若空白，调整 scope（如 `["**/*"]`、加 `requireLiteralLeadingDot:false`，或平台特定 glob），并在浏览器 devtools 看 `asset://` 请求是否 403。
- M1 之后可收紧 scope 到导入根目录（需配合运行时 scope 扩展 API），现在不做。
- capabilities 的 `core:default` 已覆盖 asset 协议权限，预期无需改 `capabilities/default.json`；若验证发现被拦再补。

## 3. 后端：`list_groups` 命令

新增 `src-tauri/src/commands/grouping.rs` 内的 query command（或新文件 `browse.rs`，与现有风格一致放 `grouping.rs` 即可），在 `lib.rs` 的 `invoke_handler` 注册。

### 3.1 DTO（camelCase serde，对齐现有 `ScanOutcome`/`AnalyzeSummary` 风格）

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePhoto {
    pub id: String,
    pub path: String,                  // 前端 api 层转 convertFileSrc，组件不见裸路径
    pub status: String,                // 'pending' | 'keep' | 'reject'
    pub shot_at: Option<String>,       // ISO8601 或 null
    pub blur_score: Option<f64>,
    pub is_blurry: Option<bool>,
    pub exposure_flag: Option<String>, // 'normal' | 'over' | 'under' | null
    pub analysis_state: String,        // 'pending' | 'done' | 'failed'
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseGroup {
    pub id: String,
    pub photos: Vec<BrowsePhoto>,      // 已按组内排序
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseModel {
    pub groups: Vec<BrowseGroup>,      // 已按组排序
    pub ungrouped: Vec<BrowsePhoto>,   // 单张 + 未分析 + 失败
}
```

### 3.2 查询与排序

并发/锁范式照搬现有命令：clone `state.db` 的 Arc → `spawn_blocking` → `blocking_lock()`，**绝不跨 `.await` 持锁**。纯 DB 读，无需 single-flight 标志。

- **组内照片**（`group_members` JOIN `photos`，限定 `similar_groups.method='phash_burst'`）：
  组内排序 = `shot_at` 升序优先、`blur_score` 降序次键、`id` 兜底。SQLite 无 `NULLS LAST`，用：
  `ORDER BY group_id, (shot_at IS NULL), shot_at ASC, (blur_score IS NULL), blur_score DESC, id`
- **组间排序**：按各组成员最早 `shot_at` 升序（无 shot_at 的组排后），次键组 `id`。一次性取出所有成员行后在 Rust 端 group-by + 排序，比多次 SQL 简单可靠。
- **未分组**：`photos p LEFT JOIN group_members gm ON p.id=gm.photo_id WHERE gm.group_id IS NULL`，同样 `shot_at` 升序、`id` 兜底。天然包含单张（连通分量=1，grouping 从不写入）、未分析、失败的照片——保证导入的照片不会在 UI 里凭空消失。

M1 数据量（数百张）一次性返回即可，不分页。

## 4. 前端架构

### 4.1 样式栈引入（Tailwind + shadcn/ui）

本项目首个真 UI 任务，落地 `frontend/component-guidelines.md` 的样式 OPEN 决策：
- 装 Tailwind + PostCSS，配 `tailwind.config.js`（`content` 指向 `index.html` + `src/**/*.{ts,tsx}`），`src/styles.css` 引 `@tailwind base/components/utilities`。
- 颜色用 **CSS 变量 → Tailwind token** 的语义层（见 §5），组件里只用语义类（`bg-surface`/`text-muted`/`border-border`），不写裸 hex（对齐 spec 的 `color-semantic`）。
- shadcn/ui 按需引组件（Badge、Button、ScrollArea 等）；可用其 MCP 检索。不全量引入。
- 暗色为默认且唯一主题（审片场景），`<html class="dark">` 固定。

### 4.2 类型扩展

`src/types/photo.ts` 现有 `Photo` 缺分析字段。新增浏览视图用类型（保持"组件不见裸路径"铁律，`src` 是 `PhotoSrc`）：

```ts
export type ExposureFlag = "normal" | "over" | "under";
export type AnalysisState = "pending" | "done" | "failed";

export type BrowsePhoto = {
  id: PhotoId;
  name: string;            // basename(path)
  src: PhotoSrc;           // convertFileSrc(path)，api 层产出
  isHeic: boolean;         // 由扩展名判定，决定占位
  status: PhotoStatus;
  shotAt: string | null;
  blurScore: number | null;
  isBlurry: boolean | null;
  exposureFlag: ExposureFlag | null;
  analysisState: AnalysisState;
};

export type BrowseGroup = { id: string; photoIds: PhotoId[] };
```

### 4.3 api 层 `src/api/groupsApi.ts`

照搬 `photosApi.ts` 的边界校验模式（Rust↔TS 无 codegen，手工 narrow）：
- `listGroups(): Promise<{ groups: BrowseGroup[]; ungroupedIds: PhotoId[]; byId: Record<PhotoId, BrowsePhoto> }>`
- 把每条原始行 `toBrowsePhoto`：`name=basename(path)`、`src=convertFileSrc(path)`、`isHeic=/\.he(ic|if)$/i.test(name)`、非法 status/exposureFlag 回落默认。
- 返回规范化结构（id 列表 + byId 字典），契合 store 形状。

### 4.4 store `src/store/groupsStore.ts`（per-domain，延续 `photosStore` 模式）

```ts
type GroupsState = {
  byId: Record<PhotoId, BrowsePhoto>;
  groups: BrowseGroup[];
  ungroupedIds: PhotoId[];
  loaded: boolean;
  load: () => Promise<void>;                       // 调 listGroups，填充
  setStatus: (id: PhotoId, status: PhotoStatus) => Promise<void>; // optimistic
  clear: () => void;
};
```

- `setStatus` **乐观更新**（spec 硬性要求）：先写 store，再 `invoke('set_status', ...)`，失败回滚。`set_status` 命令归 `keep-reject-status`；若未就绪，先只更新本地 + 留 TODO，不 invoke。
- 选择器按 id 取：`useGroupsStore((s) => s.byId[id])`，列表项**绝不读整 store**（spec `selectors`）。

### 4.5 虚拟化模型（核心难点）

需求是"组头 + 网格 + 单一滚动容器 + 虚拟化"。做法：**把分组结构扁平化成一维行数组**喂给 TanStack Virtual（装 `@tanstack/react-virtual`，新依赖）。

行类型：
```ts
type Row =
  | { kind: "header"; key: string; title: string; count: number }
  | { kind: "photos"; key: string; ids: PhotoId[] };  // 一行 COLS 张
```

构建（`useMemo`，依赖 groups/ungrouped/COLS）：每个组先出一个 `header` 行，组内照片按 COLS 切成若干 `photos` 行；最后"未分组"区同样 header + photos 行。

- `COLS` 按容器宽度算（目标瓦片宽 ~180px），用 `ResizeObserver` 监听容器宽变化重算 → 行数组重建。
- `useVirtualizer({ count: rows.length, getScrollElement, estimateSize })`：两种行高度都是常量（header 固定高、photos 行 = 瓦片高 + gap），`estimateSize` 直接返回精确值，**无需动态测量**，滚动顺滑。
- 只渲染 `virtualizer.getVirtualItems()` 命中的行 → DOM 不爆。满足 spec "‎>100 项必须虚拟化"。

### 4.6 组件拆分（函数组件 + 具名导出，`<Component>Props`，`on*` 回调）

```
GroupBrowseView         // 顶层：拿 store、建行数组、挂 virtualizer + 滚动容器
├─ GroupHeader          // 组标题行（"相似组 · N 张" / "未分组 · N 张")
├─ PhotoRow             // 一行 COLS 张瓦片（虚拟行）
│  └─ PhotoCard         // 单张瓦片（props: photo, onCompare, onSetStatus）
│     ├─ <img> | HeicPlaceholder
│     ├─ StatusPill     // pending/keep/reject，形状+文字+色
│     ├─ QualityBadges  // is_blurry / exposure over|under
│     └─ CardActions    // 进 A/B、切 status 的按钮（键盘可达）
└─ EmptyState           // 无组无照片时的空态
```

- `PhotoCard` 用 `React.memo` 包裹（列表项，profile 前不过度 memo——但此处明确是虚拟列表项，符合"列表项"豁免）；回调用 store action 引用稳定，不传内联箭头。
- 单个瓦片 `tabIndex={0}` 可聚焦，`focus-visible:ring-2`；`Enter`→`onCompare`，状态切换既有屏幕按钮也响应快捷键（与 A/B viewer 的 `1`/`2` 语义后续在 ab-compare 统一）。

## 5. 视觉设计（来自 ui-ux-pro-max）

模式 Portfolio/Image-Grid，风格中性深色"让照片说话"。语义 token（CSS 变量，注入 Tailwind）：

| Token | 值 | 用途 |
|---|---|---|
| `--background` | `#0F172A` | 画布（中性深，不偏色） |
| `--surface` (card) | `#192134` | 瓦片/卡片底 |
| `--foreground` | `#FFFFFF` | 主文字 |
| `--muted-foreground` | `#94A3B8` | 次要文字（计数、文件名） |
| `--border` | `rgba(255,255,255,0.08)` | 描边/分隔 |
| `--primary` | `#7C3AED` | 主操作（进 A/B 按钮）、focus ring |
| `--keep` | `#10B981` 绿 | 保留 |
| `--reject` | `#DC2626` 红 | 淘汰 |
| `--pending` | `#F59E0B` 琥珀 | 待定 |
| `--warn` | `#F59E0B` | is_blurry / 过曝 |
| `--info` | `#60A5FA` | 欠曝 |

字体：桌面离线应用优先 `system-ui` 栈（不引网络字体）；ui-ux-pro-max 的无障碍推荐 Atkinson Hyperlegible 列为可选，若引则本地打包，不走 CDN。

**StatusPill（三重信号，非仅颜色 — `color-not-only`）**：圆角 pill = 图标 + 文字 + 色。
- 保留：✓ + "保留"，绿底/绿描边
- 淘汰：✕ + "淘汰"，红
- 待定：○ + "待定"，琥珀
放瓦片左上角，半透明深色衬底保证压在图片上也达 4.5:1 对比。

**QualityBadges**：右上角小徽标，图标+短文。模糊="模糊"(warn)、过曝="过曝"(warn)、欠曝="欠曝"(info)。`normal` 不显示徽标。同样图标+文字，不靠纯色。

**GroupHeader**：sticky 行内左对齐标题 + 右侧灰色计数（`tabular-nums`）。相似组标题可带组序号；未分组区标题固定"未分组"。组与组之间留 24px 纵向节奏。

**PhotoCard**：`aspect-square` 瓦片，`object-cover` 居中裁切，圆角 8px，`bg-surface` 占位防 CLS（`image-dimension`：给 `<img>` 固定容器尺寸 + `loading="lazy"`）。hover/focus 150–300ms 过渡，press 轻微 scale，不改布局尺寸。CardActions 默认半隐、hover/focus 显现，但**始终键盘可达**（不靠 hover 才出现交互——`hover-vs-tap`/`gesture-alternative`）。

**HeicPlaceholder**：同尺寸瓦片，居中放图片图标 + ".HEIC" 文字 + 文件名，`bg-surface`，仍显示 StatusPill/QualityBadges/Actions，保证 HEIC 也能审。

## 6. 数据流

```
App 启动/导入分析分组完成
  → groupsStore.load()
  → invoke('list_groups')  ──spawn_blocking──> SQLite 读 + 排序
  → groupsApi 边界校验 + path→src(convertFileSrc) + isHeic
  → store: byId / groups / ungroupedIds
  → GroupBrowseView: useMemo 构建扁平 Row[]（依 COLS）
  → useVirtualizer 只渲染可见行
  → PhotoCard 选择器按 id 订阅
切 status:
  → onSetStatus → groupsStore.setStatus(id,status)
  → 乐观写 store（pill 立即变）→ invoke('set_status')[keep-reject-status]→ 失败回滚
进 A/B:
  → onCompare(groupId, photoId) → [ab-compare] 挂载点
```

何时触发 `load()`：M1 先在 `App.tsx` 提供"浏览"动作（或导入+分析+分组流程末尾）调用。事件订阅（扫描/分析进度）按 spec 在 `App.tsx` 单点 wiring——本任务不展开，仅预留。

## 7. 取舍与兼容性

- **一次性返回 vs 分页**：M1 数百张，一次返回最简；上千张再加分页/游标（M2）。
- **扁平化虚拟列表 vs 每组独立滚动**：选扁平化——单一滚动容器、组头随内容滚动、虚拟化覆盖全量；每组独立滚动容器在多组时体验破碎，否决。
- **组件不见裸路径铁律**：`BrowsePhoto.src` 是 `PhotoSrc` 分支类型，`path→src` 只在 api 层发生，编译器挡住 `<img src={rawPath}>`。
- **样式栈定调**：选 Tailwind+shadcn 会写入项目级约定，后续 ab-compare/export 等 UI 任务沿用；需同步回填 `frontend/component-guidelines.md` 的样式 OPEN 段（spec update 在 Phase 3 做）。
- **store 分区定调**：选 per-domain（新增 `groupsStore`），回填 `frontend/state-management.md` 的分区 OPEN 段。
- **向后兼容**：纯新增（命令、store、组件、依赖、conf 配置），不改既有 DB schema、不动既有命令签名。`scan_folder`/`analyze_pending`/`group_photos` 行为不变。

## 8. 运维 / 回滚

- 改动可整体回滚：`tauri.conf.json` 的 assetProtocol 段、新命令注册、新前端文件、`package.json` 新依赖（`tailwindcss`/`postcss`/`autoprefixer`/`@tanstack/react-virtual`/shadcn 相关）。无数据迁移、无破坏性 schema 变更，回滚仅是删除/还原文件。
- 最高实现风险 = §2 的 assetProtocol 在 Windows 下能否真正加载图片；implement.md 把它列为第一步 + 显式验证门。
