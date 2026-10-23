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

## Outputs

| Platform | Output | Command |
| --- | --- | --- |
| Linux | Flatpak (GNOME 51, WebKitGTK 4.1) | `just flatpak` → `dist/quarry-<version>.flatpak` |
| Linux | AppImage | `just appimage` → `dist/Quarry_<version>_amd64.AppImage` |
| Linux | `.deb` | `just deb` → `dist/Quarry_<version>_amd64.deb` |
| macOS | universal `.dmg` | `just dmg` (macOS only) → `dist/Quarry_<version>_universal.dmg` |

One command, one file, flat in `dist/`: no `.app`, no `.zip`, no checksums, no subfolders. Each build prints the full path of its output; keep that.

- `docker/linux.Dockerfile` (Ubuntu 22.04) builds the AppImage and `.deb` and runs `check-docker`. Its `export` stage is `FROM scratch` with only the bundle; `docker buildx build --output type=local,dest=dist` writes that file and nothing else.
- Caching is Docker's: toolchain and npm packages are cached layers, Cargo registry and target are BuildKit cache mounts. Never bind-mount the repo's `target/` into a build container and never write caches into the repo. Flatpak state lives in the `quarry-flatpak-cache` volume.
- Ubuntu 22.04 is the oldest distro with WebKitGTK 4.1 (Tauri's Linux guide). Do not raise it without a reason: the AppImage and `.deb` need the glibc they were built with.
- `.dmg` is built only on macOS. Tauri cannot cross-build macOS bundles; do not add a Linux workaround.
- The release profile in `Cargo.toml` is tuned for size (Tauri's size guide). Do not add dependencies or bundle resources the app does not use.

Editing a build file is not a reason to run the build. Run it only when the user asks.

## Rules

- App ID is `io.github.mindulabs.Quarry` everywhere: Tauri `identifier`, desktop entry, AppStream metainfo, Flatpak manifest, `justfile`. Change it in all places or none.
- The version comes from `[workspace.package]` in `Cargo.toml`. `package.json` and `tauri.conf.json` follow it.
- `packaging/flatpak/cargo-sources.json` is generated from `Cargo.lock` (`just flatpak`, CI). A dependency change regenerates it; do not hand-edit.
- The desktop entry, metainfo and icons live in `packaging/linux/` and `src-tauri/icons/`. CI validates them with `desktop-file-validate` and `appstreamcli`. On Wayland the dock icon comes from the desktop entry named after the app ID; `src-tauri/src/desktop.rs` installs one for unpackaged Linux runs.
- Linux has no native title bar (`decorations: false` in `tauri.linux.conf.json`); the UI draws the window controls. macOS uses an overlay title bar.
- `tauri.linux.conf.json` is merged as a JSON merge patch: its `windows` array replaces the whole array of `tauri.conf.json`. A window option for every platform goes in both files (`visible: false` did not, and Linux showed a white window at startup).
- The main window starts hidden; `ui/public/show.js` shows it once the splash in `index.html` is in the page, and `on_page_load` in `main.rs` shows it when the page has loaded, in case the script could not.
- The unpackaged Linux desktop entry (`desktop.rs`) names its icon by path, so a new logo shows without logging out; it also removes the SVG that older versions installed.
- Tauri capabilities in `src-tauri/capabilities/default.json` are least privilege: add a permission only for a window API the UI really calls. The CSP in `tauri.conf.json` stays strict (no remote origins).
- `ui/dist` must exist for `tauri::generate_context!`; CI makes an empty one for Rust jobs.

## CI

- `ci.yml` (every PR, push to `main`, and called by `release.yml`): UI, Rust on Linux (fmt, clippy, test, bindings diff), Rust on macOS (clippy, test), Integration (MySQL), cargo-deny, desktop / metainfo validation. No packaging here, so PRs stay fast. Docs-only changes (`**.md`, `docs/**`, `.claude/**`) skip it via `paths-ignore`.
- `release.yml`:
  - Tag `v*`: checks the tag equals `v` + the `Cargo.toml` version, runs `ci.yml`, builds the AppImage and `.deb` on `ubuntu-22.04`, the Flatpak in the flathub container, the universal `.dmg` on `macos-latest`, then `gh release create` publishes a public release with exactly those four files.
  - A PR that changes `release.yml`, or a manual run: builds the same packages as workflow artifacts, publishes nothing.
- Release steps: bump `version` in `Cargo.toml` (and `package.json`), commit, then push the tag `v<version>`. A mismatched tag fails the `version` job.
- Every job has `timeout-minutes`.
- Actions are pinned by commit SHA with the release in a comment (`uses: actions/checkout@<sha> # v7.0.1`). Update the SHA and comment together. `dtolnay/rust-toolchain` pinned by SHA needs `toolchain: stable` as input.
- Read the failing job's log before re-running.
- A new third-party action needs a reason.
- Do not add secrets to workflows or commit signing material.
