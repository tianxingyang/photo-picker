# Component Guidelines

Function components with hooks. No class components.

---

## Component Structure

Top-to-bottom order in every component file:

```tsx
// 1. imports (external → internal → relative)
// 2. types (Props, local types)
// 3. component (named export, no default export)
// 4. helper functions (only if used by this component and not exported)
```

**Named exports only.** No `export default` — refactors should not silently rename usages.

---

## Props Conventions

- Props type named `<Component>Props`, defined in the same file unless reused elsewhere.
- Required props first, optional last.
- Group related props as a nested object when the flat list passes ~6.
- Children typed as `React.ReactNode`, not `JSX.Element`.
- Event handlers prefixed `on*` in props (`onSelect`, `onCompareSwap`). `handle*` is the implementor side only.

---

## Photo URL Rule (project-specific)

Components MUST NOT receive raw OS file paths. The `api/` layer converts every path via `convertFileSrc` before handing it to a component. Component props for images use the type `PhotoSrc` (a branded string from `types/photo.ts`), not `string`. The compiler then refuses raw paths in `<img src>`.

---

## Styling Patterns

**DECIDED (2026-05-26, task `05-24-group-browse-ui`)**: **Tailwind v3 + shadcn/ui**.

- Tailwind utility classes for layout/spacing/state. Config in `tailwind.config.js` (`content`: `index.html` + `src/**/*.{ts,tsx}`); PostCSS in `postcss.config.js`.
- Semantic color tokens are CSS custom properties declared in `src/styles.css` `:root` (`--background`, `--surface`, `--foreground`, `--muted-foreground`, `--border`, `--primary`, `--keep`, `--reject`, `--pending`, `--warn`, `--info`) and mapped into the Tailwind theme. Components use semantic classes (`bg-surface`, `text-muted-foreground`, `border-border`) — never raw hex.
- Dark is the only theme (photo-review canvas, kept neutral so photo colour reads true); `<html class="dark">` is fixed.
- shadcn/ui components are added on demand; the `cn()` helper (`clsx` + `tailwind-merge`) lives in `src/lib/utils.ts`. Do not bulk-import shadcn.
- Rejected: CSS Modules (less leverage from shadcn/ui), vanilla-extract (heaviest config, least ecosystem leverage).

---

## Accessibility (baseline)

- Every interactive element keyboard-reachable. The A/B viewer's `1`/`2` keys for keep/reject MUST also be exposed as on-screen buttons.
- Images need `alt`. Default `alt` is the photo filename.
- Color is never the only signal — status pills combine shape/text + color.

---

## Performance

- Lists go through `TanStack Virtual`. A non-virtualized list rendering >100 items is a review-blocking smell.
- `React.memo` only after a profile proves a re-render hotspot. Premature memoization is forbidden.
- Selectors out of Zustand stores (see state-management.md) — never read the whole store inside a list item.

---

## Overlay / Modal Pattern (project-specific)

> **Established 2026-05-27** (task `05-24-ab-compare`, the app's first full-screen overlay — the A/B compare viewer). Future modals/sheets follow this.

- **App-level mount, store-driven visibility**: the overlay component renders in `App.tsx` and `return null`s when closed; open/close state lives in a per-domain Zustand store (e.g. `compareStore.open`), not lifted through props. The trigger calls a store action (`openFor(id)`); the overlay subscribes to `open`.
- **Focus trap + restore**: on open, move focus into the overlay and trap `Tab` within it so the underlying grid is not reachable; on close, **restore focus to the triggering element**. (`escape-routes` / `focus-management`.)
- **Esc + visible close**: `Esc` (via `useHotkey`, gated on `open`) and an on-screen close control both dismiss. Keyboard shortcuts that act on content (e.g. `1`/`2`) are ALSO exposed as on-screen buttons.
- **Enter/exit motion**: fade + subtle `scale(.98→1)` ~150–200ms, exit faster (~120ms), `transform/opacity` only, wrapped in `motion-safe:` so `prefers-reduced-motion` drops the movement.

## Pattern: synced transform across panes (zoom/pan)

> A single shared transform state drives BOTH panes — never per-pane state kept "in sync".

**Problem**: two side-by-side images must zoom/pan in lockstep. Two transform states + a sync effect drift and can feedback-loop.

**Solution**: one `useState<{scale,tx,ty}>` in the parent (`view` is UI-ephemeral → `useState`, not store), passed as the identical prop to both panes; each pane applies it via CSS `transform: translate(tx,ty) scale(scale)`. Reset `view` to identity on pair change / swap / close (a `useEffect` on the pair ids). Wheel zoom is anchored to the cursor: `tx' = cx - (cx - tx) * (scale'/scale)`.

**Why**: a single source of truth makes "locked" the default — drift is structurally impossible — and `transform`-only keeps it on the GPU compositor (no reflow).

## Common Mistakes

- Passing raw paths from `invoke` results into `<img src>` — fails under Tauri CSP. Must go through `convertFileSrc` in `api/`.
- Reading photo data inside `useEffect` on every render — fetch in `api/`, push into store, subscribe.
- Inline `() => ...` handlers passed into `React.memo`'d children — defeats memoization.
- Keeping two transform states "in sync" for paired views — use one shared transform (see Pattern above).

### Common Mistake: dereferencing a `ref` inside a `setState` updater closure

> Discovered 2026-05-27 (task `05-24-ab-compare`): a drag-pan handler blanked the whole app.

**Symptom**: an interaction (e.g. drag then release) throws `Cannot read properties of null` and the entire window goes blank (a render-phase throw with no error boundary unmounts the React tree → only the body background shows).

**Cause**: the `setState` **updater function runs deferred** (React 18 batches via the scheduler). If the updater dereferences a mutable `ref` that a concurrent event has since cleared, it throws — *during render*, not in the event handler.

```tsx
// ❌ Wrong — dragRef.current may be null by the time the updater runs
function onPointerMove(e) {
  if (!dragRef.current) return;            // guard runs now…
  setView((prev) => ({ ...prev, tx: dragRef.current!.startTx + dx })); // …deref runs later, after pointerup nulled it
}

// ✅ Correct — snapshot the ref into a local BEFORE setState; close over the local
function onPointerMove(e) {
  const drag = dragRef.current;
  if (!drag) return;
  setView((prev) => ({ ...prev, tx: drag.startTx + dx }));
}
```

**Prevention**: never read `someRef.current` inside a `setState`/`useState` updater closure — read it into a `const` first and capture that. The top-of-handler null guard is not enough; the value can change before the deferred updater executes.
