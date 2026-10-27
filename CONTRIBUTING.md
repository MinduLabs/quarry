# Contributing to Quarry

Thanks for your interest! Quarry is early-stage, so please open an issue or discussion before large
changes. Setting up and running the project: [README.md](README.md#development-setup).

## Contributor License Agreement

All contributors must sign the [CLA](CLA.md). The CLA Assistant bot will ask you on your first pull
request. It lets the project relicense contributions in the future (for example, commercial
licenses), while your contribution stays available under AGPL-3.0-or-later.

## Pull requests

- One logical change per PR; keep it small. Improvements you notice outside its scope go in the PR
  description, not in the diff.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`,
  `docs:`, `refactor:`, `test:`, `chore:`, `ci:`. Imperative, under 72 characters.
- A testable change ships with a test. A test you changed on purpose is named in the PR.
- CI must be green. Locally, `just check` (or `just check-docker`) runs the same checks; run
  `just test-db` when you touch the engine or a driver.
- Changed `Command`, `Event` or a `crates/model` type: run `just bindings` and commit
  `ui/src/bindings/`.
- A user-visible change adds a line under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md).
- Changed behavior or architecture: update [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- UI changes follow [docs/STYLE-GUIDE.md](docs/STYLE-GUIDE.md) and include screenshots in light and
  dark. There is no UI test runner yet; say what you checked by hand.
- A new crate or npm package needs a reason in the PR. `cargo deny` rejects bad licenses (no
  third-party GPL / AGPL) and yanked crates.

Review checks the design rules in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#principles).
