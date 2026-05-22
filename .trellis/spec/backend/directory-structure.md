# Backend Directory Structure

Two co-located trees in one repo: Rust main process under `src-tauri/`, Python sidecar under `python/`.

---

## Directory Layout

```
photo-picker/
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs            # Tauri builder, command registry, sidecar wiring
│   │   ├── commands/         # #[tauri::command] handlers, one file per domain
│   │   ├── db/               # rusqlite pool, schema accessors, migration runner
│   │   ├── scanner/          # walkdir + blake3 ingest
│   │   ├── sidecar/          # JSON-Lines IPC manager
│   │   └── error.rs          # crate-wide error type (see error-handling.md)
│   ├── migrations/           # *.sql, lexicographically ordered
│   ├── Cargo.toml
│   └── tauri.conf.json
└── python/
    ├── pyproject.toml        # uv-managed
    ├── ruff.toml
    ├── main.py               # stdin/stdout dispatch loop
    └── analyzers/            # one module per op: blur / exposure / phash / exif
```

---

## Module Organization

- `commands/` is the **only** layer Tauri talks to. Commands MUST NOT contain analysis logic — they delegate to `scanner/`, `db/`, or `sidecar/`.
- `db/` is the **only** owner of the rusqlite `Connection`. Other modules receive `&Connection` or a typed accessor.
- `sidecar/` is the **only** owner of the Python child process and the JSON-Lines stream. No other module spawns or reads from it.
- `python/analyzers/<op>.py` MUST export a callable `def run(payload: dict) -> dict`. `main.py` dispatches by op name.
- A new analysis op = new file in `python/analyzers/` + new arm in `main.py` + new `op` value documented in ARCHITECTURE.md §IPC.

---

## Naming Conventions

- Rust modules: `snake_case`, no abbreviations (`sidecar`, not `sc`).
- Python modules: `snake_case`. The op string in IPC payload is the module filename verbatim.
- Migrations: `NNNN_short_description.sql` (e.g. `0001_initial.sql`).

---

## Examples

Architectural intent in ARCHITECTURE.md §进程拓扑. As real modules land, link the canonical ones here:

- `src-tauri/src/commands/photos.rs` — first command module template (TBD).
- `python/analyzers/blur.py` — first analyzer template (TBD).
