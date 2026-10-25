# UI Style Guide

The single source of truth for how Quarry looks and moves. Read it before touching anything in `ui/`.
Tokens live in [`ui/src/styles/tokens.css`](../ui/src/styles/tokens.css); this guide explains how to use them.

## Direction

Quiet, dense, fast — a modern desktop tool.

- **Data first.** The editor and grid get the space; chrome stays thin and neutral.
- **One accent.** Neutral grays everywhere, blue only for focus, selection and the primary action.
- **Layers by background, not borders.** Sidebar, content and popovers differ by surface color; borders are 1px hairlines.
- **Dense but readable.** 13px UI text, 28px controls, 26px grid rows.
- **Motion explains, never decorates.** Short, eased, only `opacity` and `transform`.
- **Identical on macOS and Linux.** No platform-specific styling except the macOS title-bar inset.

## Rules for contributors and agents

1. Use tokens only. No hex/rgb values in components, no Tailwind arbitrary colors (`bg-[#fff]`). Need a new color? Add a token to `tokens.css` (light **and** dark) and list it below.
2. Build from `ui/src/components/ui/`. Reuse `Button`, `IconButton`, `Input`, `Select`, `Switch`, `Dialog`, `ConfirmDialog`, `Tooltip`, `Badge`, `Menu`; add a primitive there before inventing a one-off.
3. Every interactive element works with the keyboard and shows `focus-visible` styling.
4. Icons: `lucide-react`, 16px (14px in dense rows), `strokeWidth={1.75}`, color `currentColor`.
5. Animate only `opacity` and `transform` (WebKitGTK on Linux is slow at layout animations). Never `transition: all`. No `backdrop-filter`, no large blurred shadows.
6. Respect `prefers-reduced-motion` (handled globally in `base.css`; do not override).
7. Check light and dark before you finish.

## Color tokens

Use through Tailwind (`bg-surface`, `text-muted`, `border-line`, …) or `var(--…)`.

| Token (Tailwind) | Use |
|---|---|
| `bg-app` | Main content background (editor, grid) |
| `bg-sidebar` | Sidebar |
| `bg-surface` | Bars: tab bar, toolbar, grid header, status bar |
| `bg-elevated` | Popovers, menus, dialogs, toasts (with a shadow) |
| `bg-hover` / `bg-pressed` | Hover / pressed state of neutral controls and rows |
| `bg-selected` | Selected row, active tree item |
| `border-line` / `border-line-strong` | Hairlines / input borders |
| `text-fg` | Primary text |
| `text-muted` | Secondary text, labels, icons |
| `text-faint` | Placeholders, disabled, NULL — never for essential text |
| `bg-accent` / `text-accent-fg` | Primary button |
| `text-accent` | Links, accent icons |
| `bg-accent-soft` | Focus ring fill, subtle accent backgrounds |
| `text-danger`, `text-warning`, `text-success` | Status text and icons |
| `bg-danger-solid` | Destructive button fill (white text) |
| `bg-env-local`, `bg-env-staging`, `bg-env-production` | Environment badges, dots, and the 2px line along the top of a staging or production query view |
| `bg-cell-modified`, `bg-cell-inserted`, `bg-cell-deleted` | Pending grid edits |
| `bg-cell-match` | Cells matching Find in results |
| `bg-kbd`, `text-kbd-fg`, `border-kbd-border`, `kbd-edge` | Key caps (`Kbd`) |

Text/background pairs meet WCAG AA (4.5:1) except `text-faint`, which is for non-essential text only.

## Typography

| Role | Font | Size / weight | Tailwind |
|---|---|---|---|
| Body, controls | Inter Variable | 13px / 400 | `text-base` |
| Labels, metadata | Inter | 12px / 400, `text-muted` | `text-sm` |
| Captions, badges | Inter | 11px / 500–600 | `text-xs` |
| Section headings | Inter | 13px / 600 | `text-base font-semibold` |
| Dialog title | Inter | 15px / 600 | `text-lg font-semibold` |
| Empty-state title | Inter | 20px / 600 | `text-2xl font-semibold` |
| Grid cells | JetBrains Mono Variable | 12px, tabular numbers | `font-mono text-sm tabular-nums` |
| SQL editor | JetBrains Mono Variable | 13px | `font-mono text-base` |

Fonts are bundled (`@fontsource-variable/*`), so both platforms render the same.

## Spacing, size, radius

- Spacing scale: 4px steps (`1` = 4px). Common: 4, 6, 8, 12, 16, 24.
- Control height: 28px (`h-7`); small 24px (`h-6`). Icon buttons are square.
- Bars: tab bar 36px, toolbar 36px, status bar 24px. The sidebar's top row has the same height as the tab bar (36px on both platforms, which also keeps the macOS window buttons, centred about 16px from the top, level with it).
- Grid: row 26px, header 28px, cell padding 0 × 8px, row-number gutter 48px.
- Radius: 4px (`rounded-sm`) inside dense rows, 6px (`rounded-md`) controls, 8px (`rounded-lg`) popovers/cards, 12px (`rounded-xl`) dialogs.
- Shadows: `shadow-popover` for menus/tooltips, `shadow-dialog` for dialogs. Nothing else casts a shadow.

## Layout

```
┌─────────────┬────────────────────────────────────────────────┐
│ ● ● ●       │ Tab bar                                    36px │
│ Sidebar     ├────────────────────────────────────────────────┤
│ 248px       │ Toolbar (SQL): run · stop · …  ☆ ENV conn · db 36px │
│ (200–400,   ├────────────────────────────────────────────────┤
│ resizable)  │ SQL editor                                     │
│             ├──────────────── drag handle ───────────────────┤
│             │ Edit bar (editable results): + − · changes 32px │
│             ├────────────────────────────────────────────────┤
│             │ Result grid                                    │
│             ├────────────────────────────────────────────────┤
│             │ Results bar: Data|Structure · results · status · pager 36px │
└─────────────┴────────────────────────────────────────────────┘
```

- The sidebar's top row is a window drag region (`data-tauri-drag-region`). On macOS the window buttons sit at the left of that row, so it holds nothing there; on Linux the row shows the app name and the window buttons are drawn at the right of the tab bar.
- Below it: a two-way switch (Connections / Favorites) and a Filter box (32px, 8px radius).
- The sidebar hides and shows with the toggle next to the window buttons (`Ctrl/⌘ B`). It slides with a `transform`; the content area resizes at once. The toggle is drawn at the window level so it stays reachable while the sidebar is hidden.
- Above the editor, the toolbar holds what acts on SQL. Right above the grid, the edit bar (only for editable results) groups Add row, Delete rows and the pending changes with Discard / Save, where the eye is while editing. Below the grid, the results bar holds the view switch, result sets, status and pages.
- Only frequent actions get a button. Occasional ones go in the toolbar's ⋯ menu (`DropdownMenuButton`), the right-click menus and the command palette.
- Pinned tabs show a pin icon, are narrower and have no close button; they always sit before the unpinned ones.
- Every tab stays mounted, so `QueryView` and `ResultGrid` are memoized and get stable props (`useMemo` / `useCallback`): typing in one tab re-renders neither the other tabs nor the visible cells.
- Tabs stay neutral: the environment shows as the badge in the toolbar and a 2px line along the top of the query view, never as a tab color.
- No tabs open → centered empty state in the content area.

## Motion

| Token | Value | Use |
|---|---|---|
| `--duration-fast` | 120ms | Hover, press, color changes, tooltips |
| `--duration-base` | 180ms | Popovers, menus, toasts, tab open |
| `--duration-slow` | 240ms | Dialogs, sidebar collapse |
| `--ease-out` | `cubic-bezier(0.2, 0, 0, 1)` | Default for entering and moving |
| `--ease-in` | `cubic-bezier(0.4, 0, 1, 1)` | Exiting |
| `--ease-spring` | `cubic-bezier(0.16, 1, 0.3, 1)` | Dialogs, toasts (slight overshoot feel) |

Patterns (utility classes in `base.css`):

- **Popover / menu / tooltip**: fade + 4px slide from the anchor side, `duration-base`, `ease-out` (`animate-pop`).
- **Dialog**: overlay fades; panel fades and scales 0.97 → 1, `duration-slow`, `ease-spring` (`animate-dialog`).
- **Toast**: slides up 8px and fades in; fades out on dismiss (`animate-toast`).
- **Cell flash** after a successful commit: `bg-accent-soft` fading to transparent over 600ms.
- **Loading**: a 14px spinner in the toolbar or status bar after 150ms; never block the whole window.

## Components

**Buttons** — `primary` (accent fill, one per view), `secondary` (surface fill + hairline), `ghost` (transparent, toolbars), `danger` (destructive confirmations only). Text in sentence case: "New connection", "Run".

**Shortcuts** — always drawn with `Kbd`: one raised key cap per key (18px, 4px radius, a darker bottom edge, like macOS), in tooltips, the command palette, the welcome screen and Settings › Keyboard; never as plain text. Context menus show names only, no shortcuts. Every shortcut is listed once in `ui/src/lib/shortcuts.ts` with platform-neutral keys (`Mod` = ⌘ on macOS, Ctrl elsewhere); `Kbd` prints them the platform's way: `⌘ ⇧ T` with symbols in macOS order (⌃ ⌥ ⇧ ⌘), `Ctrl Shift T` on Linux. A `title` attribute uses `shortcutText()`.

**Themes** — System (follows the OS), Light or Dark, chosen in Settings. `lib/theme.ts` sets `data-theme` on the root element; the dark tokens sit under `:root[data-theme="dark"]`.

**Inputs** — 28px, `bg-app`, 1px `border-line-strong`, 6px radius, focus: accent border + 3px `accent-soft` ring. Labels above inputs, 12px muted.

**Sidebar tree** — a database lists its objects in groups with counts (Tables, Views, Procedures, Functions, Triggers, Events); a table opens to its Indexes (the primary key first) and Foreign keys, loaded when first opened. Each kind of object has its own icon color (`--icon-*` tokens, `ObjectIcon`); the label stays in text color. Rows 28px, 8px indent per level plus a 12px chevron column, 16px icon, label truncates with ellipsis. Hover `bg-hover`, active `bg-selected`. Connected connections show a 6px green dot; staging/production show a text badge (`STAGING`, `PROD`) — never color alone.

**Tab bar** — tabs 28px tall inside the 36px bar, max 220px wide, title truncates; active tab `bg-app` with a hairline, inactive `text-muted`. Running tabs show a spinner instead of the close icon.

**Data grid** — mono 12px; numbers right-aligned with tabular figures; `NULL` in italic `text-faint`; horizontal hairlines only, vertical hairlines in the header; sticky header and row-number gutter; selected cells: `bg-selected`, the active one also a 1px accent outline; long values truncate with "…" and open in full with `Shift+Enter`.

**SQL editor** — CodeMirror 6, theme built from the tokens (`ui/src/features/editor/theme.ts`). Current statement gets a faint `bg-accent-soft` band; errors get a wavy `danger` underline.

**Dialogs** — 12px radius, 24px padding, title 15px semibold, footer buttons right-aligned (secondary then primary). Destructive confirmations default focus to **Cancel**.

**Toasts** — bottom-right, `bg-elevated`, max 360px, auto-dismiss after 4s (errors stay until dismissed).

**Confirm dialog** — `ConfirmDialog`: the title asks the action ("Delete 6 rows?"), the description says what happens and what brings it back, the confirming button repeats the action in the user's words ("Delete 6 rows", not "OK"). Cancel has the focus, so Enter never confirms; the confirming button is red when data is lost or changed. In a bar of quiet buttons, a button that loses data has a red label (`danger-ghost`).

**Row numbers** — a row any selected cell touches has its number tinted, so the count on Delete matches what is highlighted.

**Environment badge** — 11px semibold uppercase on `env-*` background, white text (staging: dark text).

## Writing

- Sentence case for buttons and titles ("Test connection").
- Errors say what happened and what to do: "Couldn't connect to db.example.com: password rejected. Check the password and try again."
- Use "…" on actions that open a dialog: "Edit…".
