# CLAUDE.md

Guide for AI agents (Claude Code, Codex, …). `AGENTS.md` links here.

## Project

Quarry: an open-source database client for Linux and macOS. Tauri 2 shell, React + TypeScript UI, Rust core.
Architecture: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). UI rules: [docs/STYLE-GUIDE.md](docs/STYLE-GUIDE.md) — read before touching `ui/`.

## Layout

- `crates/` — Rust core (model, driver-api, drivers/mysql, sql, store, engine). No UI code.
- `src-tauri/` — thin bridge: `engine_send`, `engine_subscribe`, `statement_at`. No business logic.
- `ui/` — React app. `ui/src/bindings/` is generated (`just bindings`); never edit it.

## Commands

`just dev` · `just check` (or `just check-docker` on hosts without webkit2gtk-4.1) · `just db-up` + `just test-db` · `just bindings` · `just flatpak` · `just linux-bundles` · `just dmg` (macOS only) · `just mac-cross` (macOS app from Linux).

## Rules

- UI ↔ core only through `Command`/`Event` (crates/engine). Changing them → `just bindings`.
- No `unwrap()`/`expect()` outside tests. Never log passwords.
- UI: tokens only (no hex in components), primitives from `ui/src/components/ui/`, animate only opacity/transform.
- Docs and comments in English.
- Commits: Conventional Commits. **Never add `Co-Authored-By` or any AI attribution line to commits or PRs.**
