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
- An op handler module under `python/analyzers/` MUST export a callable `def run(payload: dict) -> dict`. `main.py` dispatches by op name.
- M1 ships a single `analyze` op (`analyzers/analyze.py`) that decodes each image once and **composes four algorithm modules** — `exif.py` / `blur.py` / `exposure.py` / `phash.py`, each a pure function (`extract_shot_at` / `score` / `score` / `compute`), not its own `run`. This keeps decode-once efficiency while preserving one-algorithm-per-module testability.
- A new op handler = new file exporting `run` + new arm in `main.py` + new `op` value documented in ARCHITECTURE.md §IPC. A new algorithm inside `analyze` = new pure-function module called from `analyze.py`.

---

## Naming Conventions

- Rust modules: `snake_case`, no abbreviations (`sidecar`, not `sc`).
- Python modules: `snake_case`. The op string in IPC payload is the module filename verbatim.
- Migrations: `NNNN_short_description.sql` (e.g. `0001_initial.sql`).

---

## Examples

Architectural intent in ARCHITECTURE.md §进程拓扑. As real modules land, link the canonical ones here:

- `src-tauri/src/commands/photos.rs` (`scan_folder`) / `commands/analysis.rs` (`analyze_pending`) — command module templates: clone the sidecar `Arc` under a brief lock, do DB work inside `spawn_blocking`.
- `python/analyzers/analyze.py` — op-handler template (decode once, compose `blur.py` / `exposure.py` / `phash.py` / `exif.py`).
