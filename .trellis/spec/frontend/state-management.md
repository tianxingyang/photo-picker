# State Management

Zustand. Server state via Tauri invoke + events (no React Query in MVP).

---

## State Categories

| Category | Where it lives |
|---|---|
| **UI ephemeral** (modal open, hover, focus) | `useState` in the component. |
| **Cross-component selection** (current group, selected photo, A/B pair) | Zustand store. |
| **Photo data** (lists, statuses, analysis results) | Zustand store, hydrated from Rust. |
| **Server-derived streams** (scan progress, analysis progress) | Zustand store, fed by `useTauriEvent` subscriptions wired once in `App.tsx`. |
| **URL state** | Not in MVP (single-window desktop app). |

---

## Store Partitioning

**DECIDED (2026-05-26, task `05-24-group-browse-ui`)**: **per-domain stores** (was Candidate B).

One store per domain: `photosStore` (import list — `byId`/`order`), `groupsStore` (browse model — `byId`/`groups`/`ungroupedIds`/`loaded` + optimistic `setStatus`). Add `uiStore` / `progressStore` as needs arise. Cross-store derivations are computed in selectors/components, not synced into a store body. Rationale: clear ownership and it matches the existing `photosStore`; the mega-store (one growing file) and slice pattern (extra boilerplate) were rejected for an app this size.

---

## When to Use Global State

Promote to a Zustand store when **any** of these hold:

- Two unrelated components need the same value (lift past the lowest common parent).
- The value must survive a route/view change.
- A Tauri event mutates it (events are global by nature; their landing pad must be global too).

Stay in `useState` otherwise.

---

## Selectors

- Always pass a selector: `usePhotosStore((s) => s.byId[id])`. Avoid `usePhotosStore()` — that returns the whole state and re-renders on every change.
- For object slices, use `shallow` from `zustand/shallow`.
- Derived state lives in selector functions, not in the store body. Compute on read.

---

## Actions

- Actions are methods on the store: `addPhotos(photos)`, `setStatus(id, status)`. No external setters.
- Async actions return `Promise<void>` and resolve after the underlying `invoke` + state update.
- Status changes (keep/reject/pending) MUST be optimistic — user expects instant feedback. Write to store first, then `invoke`, rollback on error.

---

## Server Sync

- One subscription per event topic, wired in `App.tsx` `useEffect`. Wiring inside child components causes duplicate handlers.
- Hydration on app start: a single `bootstrap()` action fans out the initial `invoke` calls and seeds the store.

---

## Common Mistakes

- Reading the whole store (`useStore()`) inside a list item — every state change re-renders every row. Use a per-id selector.
- Mutating store state directly (`s.byId[id].status = ...`). Zustand requires `set((s) => ({ ...s, ... }))` unless the Immer middleware is enabled — and that choice must be consistent across stores.
- Hydrating photo data outside the store (e.g. via a parallel React Query) — two sources of truth.
