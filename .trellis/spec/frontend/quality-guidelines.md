# Quality Guidelines (Frontend)

---

## Formatting

`.prettierrc` source of truth:

| Setting | Value |
|---|---|
| `printWidth` | 100 |
| `tabWidth` | 2 |
| `semi` | true |
| `singleQuote` | false (double quotes) |
| `trailingComma` | `"all"` |
| `arrowParens` | `"always"` |
| `endOfLine` | `"lf"` |

`prettier --check` runs in CI; `.prettierignore` excludes `node_modules/`, `dist/`, `src-tauri/target/`, `python/`, `*.lock`.

---

## Lint

- ESLint with `@typescript-eslint/*`, `eslint-plugin-react`, `eslint-plugin-react-hooks`. Run `--max-warnings 0`.
- Required strict-on rules beyond defaults: `@typescript-eslint/no-explicit-any: error`, `react-hooks/exhaustive-deps: error`, `react/jsx-no-bind: warn` (handler creation in render).

---

## Forbidden Patterns

- `console.log` in committed code. `console.debug` allowed only behind a `DEV` guard until a real logger lands.
- Inline `() => ...` JSX handlers passed to memoized children — defeats memoization.
- Direct `@tauri-apps/api` imports outside `src/api/`.
- Raw OS paths in component props or `<img>` src — must go through `convertFileSrc` in `api/`.
- `useEffect` whose dependency is an unstable function: either wrap with `useCallback` or move the function inside the effect body.

---

## Required Patterns

- Every Tauri command call goes through `src/api/<domain>Api.ts`.
- Every list with >100 items uses `TanStack Virtual`.
- Every Tauri event subscription returns a cleanup from its `useEffect`.
- Every async store action handles both success and failure paths; no unresolved promise rejections.

---

## Comments and Docs

- "非必要不形成" — same rule as the backend. Comments document *why*, not *what*.
- Storybook is NOT adopted in MVP. Visual review happens in the running app.
- JSDoc only on public, exported, stable utilities (today: none).

---

## Testing Requirements

- **Unit** — Vitest. `*.test.ts(x)` next to the file under test, or `src/__tests__/` for cross-cutting suites.
- **Component** — React Testing Library; assert on roles/labels, not implementation details.
- **E2E** (Milestone 2+) — Playwright driving the Tauri dev binary.

---

## Code Review Checklist

- [ ] Any `any` introduced?
- [ ] Any direct `@tauri-apps/api` import outside `api/`?
- [ ] Any new list — virtualized via TanStack?
- [ ] Any new event subscription — cleanup returned from the effect?
- [ ] Any new selector — actually a selector function, not a whole-store read?
- [ ] `prettier --check` + ESLint clean?
