# Frontend Directory Structure

Vite + React + TypeScript app under `src/`.

---

## Directory Layout

```
src/
├── main.tsx            # React root + global Tauri event wiring
├── App.tsx             # Top-level layout + routing if introduced
├── api/                # Tauri invoke wrappers, one file per Rust command domain
├── components/         # Reusable UI; subfolders for compound widgets
│   └── compare/        # A/B viewer (zoom/pan sync, keyboard 1/2)
├── store/              # Zustand stores
├── hooks/              # Custom hooks, prefixed `use`
├── types/              # Shared TS types, mirrored or generated from Rust
├── styles/             # Global CSS tokens + reset
└── assets/             # Static icons/SVGs (no runtime images here)
```

---

## Module Organization

- `api/` is the **only** module that imports from `@tauri-apps/api`. Components import named functions from `api/`; they never call `invoke` directly.
- `store/` files MUST NOT import from `components/`. Stores hold data and reducers, not React nodes.
- Photo file paths from Rust pass through `convertFileSrc` inside `api/`. Components never see raw OS paths.
- Compound components (multiple files) live in their own folder with `index.ts` re-export. Single-file components stay at top level of `components/`.

---

## Naming Conventions

- Components: `PascalCase.tsx` (`PhotoGrid.tsx`, `CompareViewer.tsx`).
- Hooks: `useThing.ts` (camelCase after `use`).
- Stores: `<domain>Store.ts` (e.g. `photosStore.ts`).
- Types: `<domain>.ts` (e.g. `photo.ts` exports `Photo`, `PhotoStatus`).
- API wrappers: `<domain>Api.ts` (e.g. `photosApi.ts`).

---

## File Co-location — OPEN

> **DECISION pending**: when a component grows past one file, layout is:
> - **Candidate A** — `ComponentName/index.tsx` + `styles.module.css` + `types.ts` + `useComponentName.ts`.
> - **Candidate B** — flat `ComponentName.tsx` + `ComponentName.module.css` + `ComponentName.types.ts` in the same folder.
> - **Candidate C** — flat until ~200 lines, then promote to folder.

---

## Examples

- A/B viewer (`components/compare/`) is the canonical compound-component placement once it lands.
- API domain pattern: `api/photosApi.ts` exports `scanFolder`, `getPhotosInGroup`, etc., all returning typed promises.
