# Type Safety

TypeScript strict. The source of truth for IPC-boundary shapes is Rust.

---

## tsconfig (required flags)

`strict: true`, `noUncheckedIndexedAccess: true`, `noImplicitOverride: true`, `exactOptionalPropertyTypes: true`.

---

## Type Organization

- IPC-boundary types live in `src/types/`. One file per domain (`photo.ts`, `group.ts`, `ipc.ts`).
- Component-local types stay in the component file.
- Hook-local types stay in the hook file.

---

## Rust ↔ TS Type Sharing — OPEN

> **DECISION pending**:
> - **Candidate A — `ts-rs`**: annotate Rust structs with `#[derive(TS)]`, `cargo test` writes `.ts` files into `src/types/generated/`. Pros: one source, automated. Cons: extra crate, build step.
> - **Candidate B — `specta` + `tauri-specta`**: generates a typed Tauri client (`src/api/generated.ts`) with full function signatures. Pros: end-to-end typed `invoke`. Cons: heavier dependency, less mature.
> - **Candidate C — hand-mirrored**: write `src/types/photo.ts` to match `photos` rows by convention. Pros: zero tooling. Cons: drifts.
>
> Until decided, every IPC wrapper in `api/` does shallow `typeof`/`in` checks on the returned object and throws a typed error on mismatch. Schema-validation libraries (Zod, Valibot) are NOT adopted in MVP.

---

## Common Patterns

### Branded types

```ts
type Brand<T, B> = T & { __brand: B };
export type PhotoId = Brand<string, "PhotoId">;
export type PhotoSrc = Brand<string, "PhotoSrc">; // returned by convertFileSrc
```

`PhotoSrc` enforces the rule from component-guidelines.md at the type level: the compiler refuses raw paths in `<img src>`.

### Discriminated unions

Use `kind` as the discriminator (matches the Rust error variant naming from error-handling.md). Prefer unions over optional fields when only one of several shapes is valid.

---

## Forbidden Patterns

- `any` — period. Use `unknown` and narrow.
- `as` assertions, except: branded primitives (`as PhotoId`), the result of a validated narrowing, and known framework gaps documented inline.
- Non-null assertions (`x!`) — narrow explicitly; an extra line is worth the safety.
- `interface` for prop types — use `type` to keep declarations from being re-opened elsewhere.

---

## Common Mistakes

- Trusting `invoke` return types because the TS signature claims so — without code generation the signature is hand-written and may lie. Generate or validate at the boundary.
- Using `Record<string, ...>` for things that need insertion order. `Map<string, ...>` preserves order; `Record` does not.
- Widening (`status: string`) where a literal union (`status: "pending" | "keep" | "reject"`) is meaningful.
