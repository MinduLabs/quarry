---
paths:
  - packaging/**
  - docker/**
  - justfile
  - .github/workflows/**
  - src-tauri/tauri.conf.json
  - src-tauri/tauri.linux.conf.json
  - src-tauri/capabilities/**
---

# Packaging, CI and release

Commands and outputs: [README.md › Packages](../../README.md#packages). Why the build looks like
this: [ARCHITECTURE.md › Packaging](../../docs/ARCHITECTURE.md#packaging).

## Builds

- One command, one file, flat in `dist/`: no `.app`, `.zip`, checksums or subfolders. Each build
  prints the full path of its output.
- Caches stay in Docker (cached layers, BuildKit cache mounts, the Flatpak volumes). Never
  bind-mount the repo's `target/` into a build container or write caches into the repo.
- Linux packages build on Ubuntu 22.04, the oldest distro with WebKitGTK 4.1. Do not raise it
  without a reason.
- `.dmg` builds only on macOS. Do not add a Linux workaround.
- The release profile is tuned for size. Do not add dependencies or bundle resources the app does
  not use.

## Identity and config

- App ID `io.github.mindulabs.Quarry` appears in the Tauri `identifier`, desktop entry, metainfo,
  Flatpak manifest and `justfile`. Change it everywhere or nowhere.
- The version lives in `Cargo.toml` `[workspace.package]` and `package.json`. Keep them equal.
- `packaging/flatpak/cargo-sources.json` is generated from `Cargo.lock`. Never hand-edit it.
- Tauri capabilities are least privilege: add a permission only for an API the UI really calls. The
  CSP allows no remote origins.

## CI

- Actions are pinned by commit SHA with the release in a comment. Update both together.
- Every job has `timeout-minutes`.
- A new third-party action needs a reason. No secrets or signing material in workflows.
- A PR that changes `release.yml` builds packages without publishing. Only a `v*` tag publishes.
- Read the failing job's log before re-running it.
