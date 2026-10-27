---
paths:
  - ui/src/**
  - biome.json
  - tsconfig.json
  - vite.config.ts
---

# UI

Look and motion: [docs/STYLE-GUIDE.md](../../docs/STYLE-GUIDE.md). This file covers how the code is
written.

## Code

- State lives in `store/app.ts`. Engine calls (`send`) live in store actions, never in components.
- `lib/engine.ts` is the only place that calls `invoke`.
- `bindings/` is generated. Never edit it.
- Build from the primitives in `components/ui/`. Add a primitive there before writing a one-off.
- A question before an action that loses data or may surprise is a `ConfirmDialog`, never a one-off
  `Dialog`.
- Enable features from `Capabilities`, never from the driver name.
- Never classify SQL in TypeScript to decide safety. The engine does that.
- Cell values are `Value` (`{t, v}`); format them through `lib/values.ts`. Never pass a 64-bit
  integer through `Number`.
- Shortcuts are declared once in `lib/shortcuts.ts`.
- TypeScript is strict with `noUncheckedIndexedAccess`: handle `undefined` from indexing.

## Performance

- Every tab stays mounted. Big components (`QueryView`, `ResultGrid`, `Sidebar`) are memoized with
  stable props: a keystroke must not re-render other tabs, the grid's cells or the tree.
- The grid and the sidebar tree stay virtualized. No per-row React state that re-renders the list;
  per-row cost must not grow with the result size.
- Animate only `opacity` and `transform` (WebKitGTK is slow at layout animation).

## Verify

- `pnpm check`, then look at it in `just dev`: light and dark, a narrow window, sidebar hidden.
- There is no UI test runner. State what you checked by hand.
