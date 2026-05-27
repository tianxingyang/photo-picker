# Hook Guidelines

---

## Naming

- Always prefix `use`. Camel-case after `use`.
- Hook files: `useThing.ts`, one named export `useThing` (no default export).
- A hook returns either a tuple, a single value, or an object — pick one shape per hook and keep it consistent.

---

## Categories

| Category | Lives in | Example |
|---|---|---|
| **State-binding** (Zustand selector wrapper) | `hooks/` next to the store it reads | `usePhotosByGroup.ts` |
| **Side-effecting** (Tauri event, keyboard) | `hooks/` | `useTauriEvent.ts`, `useHotkey.ts` |
| **Pure computation** (memoization only) | inline in the consumer; extract only when reused | — |

---

## Tauri-specific Hooks

Two patterns deserve dedicated hooks:

- `useTauriEvent<T>(name, handler)` — wraps `listen` / `unlisten`, cleans up on unmount.
- `useInvoke<T>(command, args)` — one-shot, returns `{ data, error, loading }`. Long-running progress streams go through `useTauriEvent`, not `useInvoke`.

These two hooks are the **only** way components touch `@tauri-apps/api`.

---

## Convention: `useHotkey(map, enabled)` (keyboard)

> **Established 2026-05-27** (task `05-24-ab-compare`). `src/hooks/useHotkey.ts` now exists (was previously only an example).

**What**: `useHotkey(map: Record<string, () => void>, enabled: boolean)` binds a single `keydown` listener while `enabled`, dispatching by `event.key` (`"1"`, `"ArrowLeft"`, `"Escape"`, …).

**Rules**:
- **Gate with `enabled`** — an overlay/modal passes its `open` flag so shortcuts are live only while mounted-and-active. Add/remove the listener inside the effect; don't leave a global listener running.
- **Ignore typing** — early-return when the event target is an `input` / `textarea` / `contenteditable`, so a shortcut key never eats a keystroke. (No text inputs in the A/B viewer yet, but the guard is mandatory for reuse.)
- **Cleanup** — remove the listener on unmount / when `enabled` flips false (the usual `useEffect` return).

---

## Convention: `useDisplaySrc(photo)` (HEIC display URL)

> **Established 2026-05-27** (task `05-24-ab-compare`).

A state-binding hook that resolves a renderable `PhotoSrc`: non-HEIC returns `photo.src` synchronously; HEIC calls `displayApi.transcodeForDisplay(id)` **once** and memoizes the result in a **module-level `Map<PhotoId, PhotoSrc>`** so re-opening the same photo skips the sidecar round-trip. Returns a discriminated `{ state: "loading" | "ready" | "error"; src? }` so the consumer can render skeleton / image / `HeicPlaceholder` fallback. The actual `invoke` lives in `api/` (this hook only calls the api wrapper).

---

## Data Fetching

- No remote HTTP in MVP. All "fetching" is `invoke`.
- React Query / SWR not adopted in MVP — Zustand + `useInvoke` cover the needs.
- If a query becomes cache-sensitive (refetch on focus, dedupe), revisit in Milestone 2.

---

## Common Mistakes

- Calling `invoke` directly inside a component body — re-fires on every render. Move to a hook or store action.
- Forgetting to `unlisten` a Tauri event — leaks the handler, fires after unmount, console errors.
- Returning new object identities on every render from a hook (`return { a, b }` without memo) — kills downstream `useMemo` / `React.memo`.
- Putting business logic inside a hook that only *one* component calls — inline it until reused.
