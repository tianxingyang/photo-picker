# Frontend Development Guidelines

> Best practices for frontend development in this project.

---

## Overview

This directory contains guidelines for frontend development. Fill in each file with your project's specific conventions.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | `src/` layout, `api/` isolation; co-location OPEN | Filled (1 OPEN) |
| [Component Guidelines](./component-guidelines.md) | Function components, photo-URL rule; styling: Tailwind+shadcn | Filled |
| [Hook Guidelines](./hook-guidelines.md) | Tauri-event / invoke wrappers, hook categories | Filled |
| [State Management](./state-management.md) | Zustand patterns, optimistic UI; partitioning: per-domain | Filled |
| [Quality Guidelines](./quality-guidelines.md) | Prettier + ESLint, forbidden / required patterns | Filled |
| [Type Safety](./type-safety.md) | TS strict + branded types; Rust↔TS sharing OPEN | Filled (1 OPEN) |

Frontend = React + TypeScript + Vite, talking only to the Rust main process via `@tauri-apps/api`. Source of architectural truth: README.md §技术栈 and ARCHITECTURE.md §Frontend.

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
