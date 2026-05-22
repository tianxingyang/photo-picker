# Backend Development Guidelines

> Best practices for backend development in this project.

---

## Overview

This directory contains guidelines for backend development. Fill in each file with your project's specific conventions.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | `src-tauri/` + `python/` layout, module boundaries | Filled |
| [Database Guidelines](./database-guidelines.md) | rusqlite + WAL, migrations, schema-shape OPEN | Filled (1 OPEN) |
| [Error Handling](./error-handling.md) | Three error boundaries; Rust error-lib OPEN | Filled (1 OPEN) |
| [Quality Guidelines](./quality-guidelines.md) | rustfmt + clippy + ruff; forbidden / required | Filled |
| [Logging Guidelines](./logging-guidelines.md) | stderr for Python, structured fields; library OPEN | Filled (1 OPEN) |

"Backend" here means **Rust main process + Python sidecar**, not a web backend. See README.md §技术栈 and ARCHITECTURE.md §进程拓扑 for the architectural ground truth that these guidelines distill.

---

## How to Fill These Guidelines

For each guideline file:

1. Document your project's **actual conventions** (not ideals)
2. Include **code examples** from your codebase
3. List **forbidden patterns** and why
4. Add **common mistakes** your team has made

The goal is to help AI assistants and new team members understand how YOUR project works.

---

**Language**: All documentation should be written in **English**.
