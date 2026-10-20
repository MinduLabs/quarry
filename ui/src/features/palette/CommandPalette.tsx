import * as RadixDialog from "@radix-ui/react-dialog";
import {
  Database,
  Eye,
  Search,
  Server,
  SquareFunction,
  Star,
  Table2,
  TerminalSquare,
} from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { EnvBadge } from "@/components/ui/Badge";
import { Kbd } from "@/components/ui/Kbd";
import { cn } from "@/lib/cn";
import { keysOf, type ShortcutId } from "@/lib/shortcuts";
import { type PaletteMode, useApp } from "@/store/app";

const ICON = { size: 15, strokeWidth: 1.75 } as const;

/** Matches shown at once; typing narrows the rest down. */
const SHOWN = 60;

interface Item {
  id: string;
  label: string;
  /** Second line of context: connection and database, or the command's group. */
  detail?: string;
  icon: ReactNode;
  trailing?: ReactNode;
  shortcut?: ShortcutId;
  run: () => void;
}

/**
 * Quick open (`Mod+P`): tables, views, routines and databases of the open connections, saved
 * connections and favorites. Typing `>` first (or `Mod+Shift+P`) lists commands instead.
 */
export function CommandPalette() {
  const mode = useApp((s) => s.palette);
  const setPalette = useApp((s) => s.setPalette);
  return (
    <RadixDialog.Root open={mode !== null} onOpenChange={(open) => !open && setPalette(null)}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="animate-fade fixed inset-0 z-40 bg-black/20" />
        <RadixDialog.Content
          aria-describedby={undefined}
          className="animate-dialog fixed top-[12vh] left-1/2 z-50 flex max-h-[70vh] w-[600px] max-w-[calc(100vw-32px)] -translate-x-1/2 flex-col overflow-hidden rounded-xl bg-elevated shadow-dialog outline-none"
        >
          <RadixDialog.Title className="sr-only">
            {mode === "commands" ? "Commands" : "Open"}
          </RadixDialog.Title>
          {mode && <Palette key={mode} mode={mode} />}
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

function Palette({ mode }: { mode: PaletteMode }) {
  const [text, setText] = useState(mode === "commands" ? ">" : "");
  const [index, setIndex] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const isCommands = text.startsWith(">");
  const query = (isCommands ? text.slice(1) : text).trim();
  const objects = useObjectItems();
  const commands = useCommandItems();
  const matches = useMemo(
    () => search(isCommands ? commands : objects, query).slice(0, SHOWN),
    [isCommands, commands, objects, query],
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: back to the top on every new search
  useEffect(() => setIndex(0), [text]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const choose = (item: Item | undefined) => {
    if (!item) return;
    useApp.getState().setPalette(null);
    item.run();
  };

  return (
    <>
      <div className="flex h-11 shrink-0 items-center gap-2 border-line border-b px-3">
        <Search size={15} strokeWidth={1.75} className="shrink-0 text-faint" />
        {/* The dialog focuses this first field when it opens. */}
        <input
          aria-label="Search"
          spellCheck={false}
          autoComplete="off"
          value={text}
          placeholder={isCommands ? "Run a command" : "Open a table, database or connection"}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              const step = e.key === "ArrowDown" ? 1 : -1;
              setIndex((i) => (matches.length ? (i + step + matches.length) % matches.length : 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              choose(matches[index]);
            }
          }}
          className="h-full min-w-0 flex-1 bg-transparent text-base text-fg outline-none placeholder:text-faint"
        />
        {!isCommands && (
          <span className="shrink-0 text-faint text-xs">
            Type <span className="font-mono">&gt;</span> for commands
          </span>
        )}
      </div>
      <div ref={list} role="listbox" className="min-h-0 flex-1 overflow-y-auto p-1">
        {matches.length === 0 && (
          <p className="px-3 py-6 text-center text-base text-muted">
            {isCommands ? "No matching command" : "Nothing found. Connect, or open a database."}
          </p>
        )}
        {matches.map((item, i) => (
          <div
            key={item.id}
            role="option"
            tabIndex={-1}
            data-index={i}
            aria-selected={i === index}
            onMouseMove={() => setIndex(i)}
            onClick={() => choose(item)}
            onKeyDown={() => {}}
            className={cn(
              "flex h-8 cursor-default items-center gap-2.5 rounded-md px-2 text-base",
              i === index ? "bg-accent text-accent-fg" : "text-fg",
            )}
          >
            <span className={cn("shrink-0", i === index ? "text-accent-fg" : "text-muted")}>
              {item.icon}
            </span>
            <span className="truncate">{item.label}</span>
            {item.detail && (
              <span
                className={cn(
                  "min-w-0 flex-1 truncate text-sm",
                  i === index ? "text-accent-fg/80" : "text-faint",
                )}
              >
                {item.detail}
              </span>
            )}
            {!item.detail && <span className="flex-1" />}
            {item.trailing}
            {item.shortcut && <Kbd keys={keysOf(item.shortcut)} />}
          </div>
        ))}
      </div>
    </>
  );
}

function objectIcon(kind: string) {
  if (kind === "table") return <Table2 {...ICON} />;
  if (kind === "view" || kind === "materialized_view") return <Eye {...ICON} />;
  return <SquareFunction {...ICON} />;
}

/** Everything quick open can reach. Only what is loaded: open databases' objects. */
function useObjectItems(): Item[] {
  const connections = useApp((s) => s.connections);
  const nodes = useApp((s) => s.nodes);
  const favorites = useApp((s) => s.favorites);
  return useMemo(() => {
    const { openObject, toggleConnection, toggleDatabase, openFavorite } = useApp.getState();
    const items: Item[] = [];
    for (const config of connections) {
      const node = nodes[config.id];
      for (const db of node?.databases ?? []) {
        for (const object of db.objects ?? []) {
          items.push({
            id: `o:${config.id}:${db.name}:${object.kind}:${object.name}`,
            label: object.name,
            detail: `${config.name} · ${db.name}`,
            icon: objectIcon(object.kind),
            run: () => openObject(config.id, db.name, object.name, object.kind),
          });
        }
        items.push({
          id: `d:${config.id}:${db.name}`,
          label: db.name,
          detail: config.name,
          icon: <Database {...ICON} />,
          run: () => {
            if (!db.expanded) toggleDatabase(config.id, db.name);
          },
        });
      }
    }
    for (const favorite of favorites) {
      items.push({
        id: `f:${favorite.id}`,
        label: favorite.name,
        detail:
          favorite.item.kind === "query"
            ? "Saved query"
            : favorite.item.kind === "table"
              ? "Favorite table"
              : "Favorite connection",
        icon: <Star {...ICON} />,
        run: () => openFavorite(favorite),
      });
    }
    for (const config of connections) {
      const connected = nodes[config.id]?.status.kind === "connected";
      items.push({
        id: `c:${config.id}`,
        label: config.name,
        detail: connected ? "Connected" : "Connect",
        icon: <Server {...ICON} />,
        trailing: <EnvBadge environment={config.environment} />,
        run: () => {
          if (!connected) toggleConnection(config.id);
        },
      });
    }
    return items;
  }, [connections, nodes, favorites]);
}

function useCommandItems(): Item[] {
  const activeTab = useApp((s) => s.tabs.find((t) => t.id === s.activeTabId));
  const canReopen = useApp((s) => s.closedTabs.length > 0);
  return useMemo(() => {
    const s = useApp.getState();
    const icon = <TerminalSquare {...ICON} />;
    const items: Item[] = [
      {
        id: "newQuery",
        label: "New query",
        icon,
        shortcut: "newQuery",
        run: () => s.newQuery(),
      },
      {
        id: "newConnection",
        label: "New connection…",
        icon,
        shortcut: "newConnection",
        run: () => s.openDialog({ mode: "new" }),
      },
      {
        id: "settings",
        label: "Settings…",
        icon,
        shortcut: "settings",
        run: () => s.setSettingsOpen(true),
      },
      {
        id: "sidebar",
        label: "Show or hide the sidebar",
        icon,
        shortcut: "toggleSidebar",
        run: s.toggleSidebar,
      },
      {
        id: "history",
        label: "Query history…",
        icon,
        shortcut: "history",
        run: () => s.setHistoryOpen(true),
      },
      { id: "light", label: "Theme: light", icon, run: () => s.setTheme("light") },
      { id: "dark", label: "Theme: dark", icon, run: () => s.setTheme("dark") },
      { id: "system", label: "Theme: follow the system", icon, run: () => s.setTheme("system") },
    ];
    if (canReopen) {
      items.push({
        id: "reopen",
        label: "Reopen closed tab",
        icon,
        shortcut: "reopenTab",
        run: s.reopenClosedTab,
      });
    }
    if (activeTab) {
      const id = activeTab.id;
      items.push(
        {
          id: "refresh",
          label: "Refresh results",
          detail: activeTab.title,
          icon,
          shortcut: "refresh",
          run: () => s.refresh(id),
        },
        {
          id: "pin",
          label: activeTab.pinned ? "Unpin tab" : "Pin tab",
          detail: activeTab.title,
          icon,
          run: () => s.togglePin(id),
        },
        {
          id: "close",
          label: "Close tab",
          detail: activeTab.title,
          icon,
          shortcut: "closeTab",
          run: () => s.closeTab(id),
        },
      );
      const browse = activeTab.browse;
      items.push({
        id: "export",
        label: "Export…",
        detail: activeTab.title,
        icon,
        run: () =>
          s.transfer(
            "export",
            browse && activeTab.paged
              ? { kind: "table", conn: activeTab.connId, table: browse.table }
              : { kind: "result", tab: id },
          ),
      });
      if (browse) {
        items.push({
          id: "import",
          label: "Import…",
          detail: activeTab.title,
          icon,
          run: () =>
            s.transfer("import", { kind: "table", conn: activeTab.connId, table: browse.table }),
        });
      }
      if (activeTab.running) {
        items.push({ id: "stop", label: "Stop the query", icon, run: () => s.cancel(id) });
      }
      if (activeTab.browse) {
        items.push(
          {
            id: "data",
            label: "Show data",
            detail: activeTab.title,
            icon,
            run: () => s.setView(id, "data"),
          },
          {
            id: "structure",
            label: "Show structure",
            detail: activeTab.title,
            icon,
            run: () => s.setView(id, "structure"),
          },
          {
            id: "count",
            label: "Count rows exactly",
            detail: activeTab.title,
            icon,
            run: () => s.countRows(id),
          },
        );
      }
      items.push(
        activeTab.txOpen
          ? {
              id: "commit",
              label: "Commit transaction",
              icon,
              run: () => s.endTransaction(id, true),
            }
          : { id: "begin", label: "Begin transaction", icon, run: () => s.beginTransaction(id) },
      );
      if (activeTab.txOpen) {
        items.push({
          id: "rollback",
          label: "Roll back transaction",
          icon,
          run: () => s.endTransaction(id, false),
        });
      }
    }
    return items;
  }, [activeTab, canReopen]);
}

/**
 * Items whose label contains the query's letters in order, best first: a match at the start, then
 * a whole substring, then letters spread out. The detail is searched too, at a lower rank.
 */
function search(items: Item[], query: string): Item[] {
  if (!query) return items;
  const q = query.toLowerCase();
  const scored: Array<{ item: Item; score: number }> = [];
  for (const item of items) {
    const label = item.label.toLowerCase();
    let score = scoreOf(label, q);
    if (score === null && item.detail) {
      const detail = scoreOf(`${label} ${item.detail.toLowerCase()}`, q);
      score = detail === null ? null : detail + 1000;
    }
    if (score !== null) scored.push({ item, score });
  }
  scored.sort((a, b) => a.score - b.score || a.item.label.length - b.item.label.length);
  return scored.map((s) => s.item);
}

/** Lower is better; null when the letters are not all there in order. */
function scoreOf(text: string, q: string): number | null {
  const at = text.indexOf(q);
  if (at === 0) return 0;
  if (at > 0) return 10 + at;
  let pos = -1;
  let gaps = 0;
  for (const ch of q) {
    const next = text.indexOf(ch, pos + 1);
    if (next < 0) return null;
    gaps += next - pos - 1;
    pos = next;
  }
  return 100 + gaps;
}
