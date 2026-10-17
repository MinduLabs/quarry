---
paths:
  - ui/src/**
  - biome.json
  - tsconfig.json
  - vite.config.ts
---

# UI

Read [docs/STYLE-GUIDE.md](../../docs/STYLE-GUIDE.md) first. It is the source of truth for look and motion. This file covers how the code is organized.

## Structure

- `ui/src/features/<name>/`: one folder per feature (`sidebar`, `connections`, `tabs`, `editor`, `query`, `grid`, `welcome`, `window`).
- `ui/src/components/ui/`: shared primitives (`Button`, `IconButton`, `Input`, `Select`, `Switch`, `Dialog`, `Tooltip`, `Badge`, `Menu`, `Kbd`, `Toaster`, `Spinner`, `ErrorNotice`, `SegmentedTabs`, `TreeRow`). Add a primitive here before a one-off.
- `ui/src/store/app.ts`: zustand store. `handleEvent` reduces engine events; actions call `send`. Components read state and call actions.
- `ui/src/lib/engine.ts`: the only place that calls `invoke`.
- `ui/src/styles/tokens.css`: design tokens (light and dark). `base.css`: global styles and animation utilities.
- `ui/src/bindings/`: generated. Never edit.

## Rules

- Tokens only: no hex / rgb in components, no Tailwind arbitrary colors. A new color is a token in both themes.
- Build from primitives. Icons are `lucide-react`, 16px (14px dense), `strokeWidth={1.75}`.
- Animate only `opacity` and `transform`. Never `transition: all`, `backdrop-filter` or large blurred shadows. Do not override the global `prefers-reduced-motion` handling.
- Every interactive element works by keyboard and shows `focus-visible`.
- Never branch on the driver name. Enable features from `Capabilities`.
- Never classify SQL in TypeScript to decide safety. The engine does that (`ConfirmRequired`).
- Never keep engine calls in components. Put them in a store action.
- Cell values are `Value` (`{t, v}`); format them through `lib/values.ts`. Do not stringify a 64-bit integer through `Number`.
- Large lists (grid rows, tree) stay virtualized. Per-row cost must not grow with the result size.
- Strings: sentence case, "…" for actions that open a dialog, shortcuts drawn with `Kbd`.
- TypeScript is strict with `noUncheckedIndexedAccess`; handle `undefined` from indexing. No `any`. Imports use `@/`.

## Verify

```bash
pnpm check        # Biome + tsc
pnpm build        # tsc + vite build
just dev          # look at it: light and dark, narrow window (min 760x480), sidebar hidden
```

There is no UI test runner. State what you checked by hand. A UI PR needs light and dark screenshots.
