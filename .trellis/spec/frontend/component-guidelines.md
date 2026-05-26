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

## Common Mistakes

- Passing raw paths from `invoke` results into `<img src>` — fails under Tauri CSP. Must go through `convertFileSrc` in `api/`.
- Reading photo data inside `useEffect` on every render — fetch in `api/`, push into store, subscribe.
- Inline `() => ...` handlers passed into `React.memo`'d children — defeats memoization.
