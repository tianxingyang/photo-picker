# Design — A/B 对比 Viewer

> Task: `05-24-ab-compare` ｜ Parent: `05-24-milestone-1-mvp`
> 技术设计。需求与验收见 `prd.md`；执行清单见 `implement.md`。

## 1. 范围与边界

交付一个 **App 级全屏 overlay** 的双图对比器：左右并排两张照片，缩放/平移锁定同步，底部组内胶片条选 B/换 A，键盘 `1`/`2`（+屏幕按钮）给 A/B 打 `keep`，Esc 关闭。HEIC 经 **Python sidecar 按需转码**为临时 JPEG 后显示（D5）。

边界（明确不做 / 复用既有）：
- **status 落库命令**：keep-reject-status 已交付，`set_status` 命令 + `groupsStore.setStatus`（乐观、单飞、按 id 回滚）已就绪。本任务**直接调 `groupsStore.setStatus`**，不新写落库逻辑。
- **资产协议**：group-browse-ui 已启用 `assetProtocol.enable + scope ["**"]`，覆盖 OS temp 目录，转码产物可直接 `convertFileSrc`。本任务不动 `tauri.conf.json`。
- **组数据/分组**：组成员来自已就绪的 `groupsStore`（`byId`/`groups`/`ungroupedIds`）。本任务不查 DB 取组。
- **叠加/闪烁对比、差异高亮、>2 图对比**：M2+。
- **HEIC 占位升级**：group-browse 网格的 `HeicPlaceholder` 是否改走转码真显示——本任务只交付 viewer 内的真显示路径；网格升级可作为后续小改（非本任务验收项）。

## 2. 后端：HEIC 转码（D5）

### 2.1 Python `transcode` op（新增 `python/analyzers/transcode.py`）

与现有 analyzer 同构：`run(payload: dict) -> dict`，不 catch 异常（`main.handle` 统一兜成 `{id,error}`），不写 stdout。

```python
from __future__ import annotations
import os
import pillow_heif
from PIL import Image

pillow_heif.register_heif_opener()  # 幂等；自包含，不依赖 analyze 先 import

# 显示用上限：约束转码产物分辨率，平衡可缩放清晰度与内存（24MP HEIC -> ~4096 长边）
DISPLAY_MAX_SIDE = 4096
JPEG_QUALITY = 90

def run(payload: dict) -> dict:
    src = payload["path"]
    dest = payload["dest"]                       # Rust 算好的缓存目标路径
    max_side = max(1, int(payload.get("maxSide", DISPLAY_MAX_SIDE)))
    with Image.open(src) as img:
        img = img.convert("RGB")                 # 去 alpha/HDR，JPEG 安全
        longest = max(img.width, img.height)
        if longest > max_side:
            scale = max_side / longest
            img = img.resize((max(1, round(img.width*scale)), max(1, round(img.height*scale))), Image.Resampling.BILINEAR)
        tmp = dest + ".part"                      # 先写临时名再 rename，避免半成品被读
        img.save(tmp, format="JPEG", quality=JPEG_QUALITY)
        os.replace(tmp, dest)
    return {"dest": dest, "width": img.width, "height": img.height}
```

注册（`python/main.py` 的 `OPS` + `python/analyzers/__init__.py`）：

```python
# main.py
from analyzers import analyze, echo, transcode
OPS = {"echo": echo.run, "analyze": analyze.run, "transcode": transcode.run}
```

> 注意 `CALL_TIMEOUT=30s`（sidecar/mod.rs）：单张 HEIC 转码远在其内。

### 2.2 Rust 命令 `transcode_for_display`（`src-tauri/src/commands/photos.rs` 内新增，`lib.rs` 注册）

```rust
#[tauri::command]
pub async fn transcode_for_display(photo_id: String, state: State<'_, AppState>) -> Result<String, AppError> {
    // 1. 查源路径（组件不见裸路径——前端只传 id）
    let path = /* spawn_blocking + blocking_lock 查 photos.path WHERE id=?；缺失 -> AppError::NotFound */;
    // 2. 缓存键：源路径 + mtime（源改了就重转）。blake3 已是依赖（组 id 用）
    let mtime = std::fs::metadata(&path)?.modified()?;       // -> 转 u128 nanos
    let key = blake3::hash(format!("{path}|{mtime_nanos}").as_bytes()).to_hex();
    let dest = cache_dir().join(format!("{key}.jpg"));        // std::env::temp_dir()/photo-picker-display/
    // 3. 命中即返回，不重转
    if dest.exists() { return Ok(dest.to_string_lossy().into_owned()); }
    std::fs::create_dir_all(dest.parent().unwrap())?;
    // 4. 调 sidecar（克隆 Arc 后释放锁再 await，照搬 echo_via_sidecar 范式）
    let sidecar = { state.sidecar.lock().await.as_ref().cloned()
        .ok_or_else(|| AppError::Sidecar("not started".into()))? };
    match sidecar.call("transcode", json!({ "path": path, "dest": dest })).await {
        Ok(Ok(_)) => Ok(dest.to_string_lossy().into_owned()),
        Ok(Err(e)) => Err(AppError::Sidecar(e)),             // op 级（坏图）-> 前端回退 HeicPlaceholder
        Err(e) => Err(AppError::Sidecar(e.to_string())),     // 传输级
    }
}
```

- 返回**临时文件路径字符串**；前端 api 层 `convertFileSrc` 成 `PhotoSrc`（守住「组件不见裸路径」铁律）。
- 缓存生命周期（M1 取舍）：写 OS temp，**不主动清理**——OS 周期清 temp 即可；按 `path+mtime` 派生保证源改后失效、同会话重开命中。主动清理（启动清旧/LRU）属 M2。
- 错误分层沿用 echo 范式：`Ok(Err)`=坏图（前端回退占位），`Err`=sidecar 故障（前端提示+回退）。

## 3. 前端架构

### 3.1 选择/对比状态 → 新增 `src/store/compareStore.ts`（per-domain，state-management 规定「A/B pair → store」）

```ts
type CompareState = {
  open: boolean;
  memberIds: PhotoId[];          // 当前组成员（胶片条数据源），孤立张则 [id]
  aId: PhotoId | null;
  bId: PhotoId | null;           // 孤立组为 null
  openFor: (photoId: PhotoId) => void;  // 由 id 反查 groupsStore 取组，A=点击张、B=下一张
  setB: (id: PhotoId) => void;
  swap: () => void;              // A <-> B
  stepB: (dir: 1 | -1) => void;  // 方向键在 memberIds 内步进 B（跳过 == A 的张）
  close: () => void;
};
```

- `openFor` 跨 store 读：`useGroupsStore.getState()` 找含 `photoId` 的组（`groups[].photoIds` 命中则 memberIds=该组；否则该 id 落在 `ungroupedIds` → memberIds=[id], bId=null）。
- 纯选择 + 开关状态，不持有照片数据本体（数据仍在 `groupsStore.byId`，按 id 订阅）。
- keep 不进 compareStore：直接调 `groupsStore.setStatus(id,'keep')`（已就绪、乐观）。

### 3.2 显示 URL 解析 → 新增 hook `src/hooks/useDisplaySrc.ts`

非 HEIC：直接返回 `photo.src`（已是 `groupsApi` 产出的 `PhotoSrc`）。
HEIC：调 `transcodeForDisplay(id)` 一次，返回 `{ state: 'loading'|'ready'|'error', src? }`；模块级 `Map<PhotoId, PhotoSrc>` 缓存，避免重复转码往返。封装唯一触达 `@tauri-apps/api` 的入口走 api 层。

### 3.3 api 层 `src/api/displayApi.ts`（沿用 photosApi 边界校验风格）

```ts
export async function transcodeForDisplay(id: PhotoId): Promise<PhotoSrc> {
  const dest = await invoke<unknown>("transcode_for_display", { photoId: id });
  if (typeof dest !== "string" || dest.length === 0) throw new Error("transcode_for_display: bad shape");
  return convertFileSrc(dest) as PhotoSrc;     // path -> asset URL，scope ["**"] 已覆盖 temp
}
```

### 3.4 键盘 → 新增 hook `src/hooks/useHotkey.ts`（hook-guidelines 列为示例 side-effecting hook）

`useHotkey(map, enabled)`：`enabled` 时绑 `keydown`，卸载/禁用时解绑。viewer 打开时注册 `1/2/ArrowLeft/ArrowRight/Escape`。`1`→keep A、`2`→keep B、`←/→`→`stepB`、`Esc`→`close`。输入框聚焦时忽略（本 overlay 无输入框，仍按 spec 防御）。

### 3.5 同步缩放/平移（核心难点）

变换是 **UI ephemeral**（state-management）→ 留在 `ABCompareViewer` 的 `useState`，**单一 transform 同时驱动两个 pane**（这就是「锁定同步」的实现）：

```ts
type View = { scale: number; tx: number; ty: number };   // 共享
// 滚轮：以光标为锚缩放（scale 在 [1, 8] 夹紧）；拖拽：改 tx/ty
// 两个 ComparePane 同时收到同一个 view，<img> 用 transform: translate(tx,ty) scale(scale)
```

- 只用 `transform`（GPU 合成，`transform-performance`），不动 width/height/top/left，不触发 reflow。
- 切换 pair / swap / 关闭 → `view` 归位到 `{1,0,0}`。
- 缩放锚定光标：`tx' = cx - (cx - tx) * (scale'/scale)`（两轴同理）。

### 3.6 组件拆分（函数组件 + 具名导出，`<Component>Props`，`on*` 回调）

```
ABCompareViewer            // App 级 overlay；读 compareStore.open；持 view 状态 + useHotkey
├─ ComparePane (×2)        // 接收 photoId + view + side('left'|'right')；用 useDisplaySrc 解析 src
│   └─ <img> | <CompareLoading/>（转码中）| <HeicPlaceholder/>（转码失败回退）
├─ CompareToolbar          // 顶部：缩放 +/-/重置、关闭(×, Esc)；右上文件名 + 缩放%(tabular-nums)
├─ CompareFilmstrip        // 底部：组内缩略图（按 id 订阅 byId）；点击设 B；A/B 高亮；A↔B 互换钮
└─ CompareKeepBar          // 「保留左 (1)」「保留右 (2)」按钮（图标+文字+色），等效键盘 1/2
```

- 挂载点：`App.tsx` 渲染 `<ABCompareViewer/>`（`open=false` 时 return null），`onCompare(id)` 改为 `compareStore.getState().openFor(id)`，删掉占位 `setNotice`。
- 列表项（胶片条缩略图）按 id 选择器订阅，不读整 store；`React.memo` 仅在 profile 证明热点后加（spec 禁过早 memo——胶片条量小，先不 memo）。

## 4. 视觉设计（来自 ui-ux-pro-max）

风格 **Dark Mode (OLED)**，与 group-browse 同一套语义 token（复用 `src/styles.css` 已声明的 CSS 变量，不新增裸 hex）。核心原则「let the photos speak」：画布近黑、chrome 极简、控件 hover/focus 才显著、图片占绝对主导。

布局（全屏 overlay，`z-50`，`fixed inset-0`）：
```
┌──────────────────────────────────────────────────────────┐
│ [×]                       1 / 4 · IMG_0423.HEIC      [-][%][+] │  CompareToolbar（半透明，悬浮）
├───────────────────────────┬──────────────────────────────┤
│                           │                              │
│        A（左）pane         │        B（右）pane            │  并排各 50%，bg=#0F172A
│   <img> 同步 transform     │   <img> 同步 transform        │  中缝 1px --border 分隔
│                           │                              │
├───────────────────────────┴──────────────────────────────┤
│   [ ✓ 保留左 (1) ]              [ 保留右 (2) ✓ ]            │  CompareKeepBar
├──────────────────────────────────────────────────────────┤
│  ▢ ▢ [▣A] ▢ [▣B] ▢ ▢   ⇄        缩略图胶片条（横向滚动）    │  CompareFilmstrip
└──────────────────────────────────────────────────────────┘
```

| 区域 | 视觉规格 |
|---|---|
| **Overlay 画布** | `bg-background`(#0F172A) 不透明全覆盖；进场 150–200ms `fade + scale(.98→1)`，出场更快(~120ms)；`prefers-reduced-motion` 时取消位移只留 opacity。`modal-motion`/`exit-faster-than-enter`。 |
| **ComparePane** | 各占 50% 宽、占满可用高；图片 `object-contain` 居中（不裁切，审片要看全幅）；`bg-background`；图片容器固定尺寸防 CLS（`image-dimension`）。中缝 `1px` `--border`。 |
| **CompareToolbar** | 顶部悬浮条，`bg-surface/80 backdrop-blur`，默认低调、hover 区域提亮。左：关闭 `×`（Lucide，44×44 命中区，`aria-label="关闭对比 (Esc)"`）。中：`N / total · 文件名`（`text-muted-foreground`，文件名 `truncate`）。右：缩放 `−` / 百分比(`tabular-nums`) / `+` 与「重置」。图标统一 Lucide、stroke 1.5。 |
| **CompareKeepBar** | 两个对称主按钮。保留左：`✓`+「保留左 (1)」，已选则实心 `--keep`(#10B981) 底；保留右同理。**三重信号**（图标✓+文字+绿色，`color-not-only`）。按钮 ≥44px 高，`focus-visible:ring-2 ring-primary`，press `scale .97`。当前张已是 keep 时按钮呈「已保留」实心态。 |
| **CompareFilmstrip** | 底部横向滚动条，缩略图 `aspect-square` ~64px，圆角 6px，`bg-surface`。当前 **A** 描边 `ring-2 ring-primary` + 左上角标「A」；**B** 描边 `ring-2 ring-keep`? 否——为避免与 keep 语义混淆，A/B 均用 `--primary` 描边，靠**角标 A/B 文字**区分（`color-not-only`）。点击=设 B；hover 提亮；键盘可 Tab 聚焦、Enter 设 B。右侧 `⇄` 互换按钮（`aria-label="左右互换"`）。HEIC 缩略图同样走占位/小图。 |
| **CompareLoading（HEIC 转码中）** | pane 内居中 skeleton/shimmer（>300ms 才显，`progressive-loading`），文案「解码中…」+ 文件名；不是空白、不闪烁。 |
| **HeicPlaceholder 回退** | 转码失败复用既有 `HeicPlaceholder` 组件 + 一行错误提示「无法解码此 HEIC」。 |
| **焦点管理** | 打开时焦点移入 overlay（关闭按钮或 keep bar）；overlay 是焦点陷阱（Tab 不逃逸到底层 grid）；关闭后焦点还原到触发瓦片（`escape-routes`/`focus-management`）。 |

字体：延续 group-browse 的 `system-ui` 栈（离线桌面不引 CDN 字体）；ui-ux-pro-max 推荐的 Atkinson Hyperlegible 仅作可选、若引则本地打包。
图标：Lucide（SVG，禁 emoji，stroke 1.5 统一）。
动效：所有过渡 150–300ms，`transform/opacity` only，统一 easing；尊重 `prefers-reduced-motion`。

## 5. 数据流

```
网格点击瓦片
  → App.onCompare(id) → compareStore.openFor(id)
      → 读 groupsStore.getState() 定位组 → memberIds / aId=id / bId=下一张
  → <ABCompareViewer open> 渲染
      → ComparePane(A/B) → useDisplaySrc(photo)
            非HEIC: photo.src 直接用
            HEIC:   displayApi.transcodeForDisplay(id)
                      → invoke('transcode_for_display',{photoId})
                      → Rust 查 path → 缓存命中? 否则 sidecar.call('transcode')
                      → temp jpg path → convertFileSrc → PhotoSrc → <img>
      → 滚轮/拖拽 → 单一 view 状态 → 两 pane 同步 transform
胶片条点击/方向键
  → compareStore.setB(id) / stepB(±1) → view 归位
按 1/2（或 KeepBar 按钮）
  → groupsStore.setStatus(aId|bId, 'keep')  // 乐观、单飞、按 id 回滚（既有）
  → pane 状态标即时反映；D4：只改当前张
Esc / 关闭
  → compareStore.close() → overlay 卸载 → 焦点还原触发瓦片
```

## 6. 取舍与兼容性

- **sidecar 转码 vs Rust/wasm**（D5）：选 sidecar——pillow-heif 已是依赖、零新依赖、与 JSON-Lines 架构同构；代价首次转码延迟，用 path+mtime 缓存抵消。
- **单一 view 驱动两 pane vs 两套独立 view + 同步**：选单一共享 view——天然「锁定」，无同步漂移/回环风险，最简。
- **compareStore vs 塞进 groupsStore**：选独立 store——选择/对比是 UI 选择域，per-domain 分区（state-management 既定），groupsStore 专注照片数据。
- **keep 直接调 groupsStore.setStatus vs 回调上抛**：keep-reject-status 已交付，直接复用既有乐观落库，不再造回调层。PRD 原写「emit 选择事件」是其规划时 keep 尚未就绪的措辞，现以「复用 setStatus」取代。
- **转码缓存不主动清理**：M1 取舍，OS temp 自清；LRU/启动清旧属 M2。
- **`object-contain` vs `cover`**：审片选 contain 看全幅；网格瓦片才用 cover。
- **向后兼容**：纯新增（python op、Rust 命令、前端 store/hook/api/组件、`App.tsx` 接线）；不改 DB schema、不动既有命令签名、不动 `tauri.conf.json`。`onCompare(id)` 签名不变。

## 7. 运维 / 回滚

- 改动可整体回滚：删 `transcode.py` + 还原 `main.py`/`__init__.py` 注册；删 Rust `transcode_for_display` + 还原 `lib.rs` 注册；删前端 `compareStore`/`useDisplaySrc`/`useHotkey`/`displayApi`/`components/compare/*`；还原 `App.tsx` 的 `onCompare` 占位。无数据迁移、无破坏性变更。
- 最高实现风险：① HEIC 转码在 Windows + 真实大图的端到端（sidecar 路径、temp 写权限、`convertFileSrc` 对 temp 路径生效）；② 大图双开的内存/流畅度。implement.md 把这两点列为显式验证门。
