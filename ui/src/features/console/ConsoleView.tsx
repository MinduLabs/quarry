import { MySQL } from "@codemirror/lang-sql";
import { highlightCode } from "@lezer/highlight";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Copy, Trash2 } from "lucide-react";
import { memo, type ReactNode, useEffect, useMemo, useRef } from "react";
import type { LogEntry } from "@/bindings/LogEntry";
import type { LogSource } from "@/bindings/LogSource";
import { IconButton } from "@/components/ui/IconButton";
import { ContextMenuArea } from "@/components/ui/Menu";
import { copyText, useApp } from "@/store/app";
import { sqlHighlightStyle } from "../editor/theme";

const ICON = { size: 14, strokeWidth: 1.75 } as const;

const SOURCE_LABEL: Record<LogSource, string | null> = {
  query: null,
  page: "table page",
  edit: "grid edit",
  transaction: "transaction",
};

/**
 * What the tab's session ran, oldest first, read like a SQL file: a comment line with the time,
 * the outcome and how long it took, then the statement in editor colors. Follows the end while
 * it is in view.
 */
export const ConsoleView = memo(function ConsoleView({
  tab,
  log,
}: {
  tab: number;
  log: LogEntry[];
}) {
  const scroller = useRef<HTMLDivElement>(null);
  /** The view sits at the end, so new entries keep it there. */
  const atEnd = useRef(true);
  const virtualizer = useVirtualizer({
    count: log.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 56,
    overscan: 8,
  });

  useEffect(() => {
    if (atEnd.current && log.length > 0) {
      virtualizer.scrollToIndex(log.length - 1, { align: "end" });
    }
  }, [log.length, virtualizer]);

  return (
    <div className="flex h-full min-h-0 flex-col bg-app">
      <div className="flex h-8 shrink-0 items-center gap-1 border-line border-b bg-surface pr-2 pl-3">
        <span className="text-muted text-sm tabular-nums">
          {log.length === 1 ? "1 statement" : `${log.length.toLocaleString()} statements`}
        </span>
        <div className="flex-1" />
        <IconButton
          label="Copy all"
          size="sm"
          disabled={log.length === 0}
          onClick={() => copyText(log.map(entryText).join("\n\n"), "Copied the output")}
        >
          <Copy {...ICON} />
        </IconButton>
        <IconButton
          label="Clear"
          size="sm"
          disabled={log.length === 0}
          onClick={() => useApp.getState().clearLog(tab)}
        >
          <Trash2 {...ICON} />
        </IconButton>
      </div>
      {log.length === 0 ? (
        <p className="flex flex-1 items-center justify-center text-base text-faint">
          Statements this tab runs show here
        </p>
      ) : (
        <div
          ref={scroller}
          onScroll={(e) => {
            const el = e.currentTarget;
            atEnd.current = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
          }}
          className="min-h-0 flex-1 select-text overflow-auto py-2"
        >
          <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((item) => {
              const entry = log[item.index];
              if (!entry) return null;
              return (
                <div
                  key={item.key}
                  data-index={item.index}
                  ref={virtualizer.measureElement}
                  className="absolute inset-x-0 top-0"
                  style={{ transform: `translateY(${item.start}px)` }}
                >
                  <Entry entry={entry} />
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
});

const Entry = memo(function Entry({ entry }: { entry: LogEntry }) {
  // Ended with `;` like a script, so entries read as one SQL file.
  const sql = useMemo(() => highlighted(terminated(entry.sql)), [entry.sql]);
  return (
    <ContextMenuArea
      items={[
        { label: "Copy statement", onSelect: () => copyText(entry.sql) },
        { label: "Copy entry", onSelect: () => copyText(entryText(entry)) },
      ]}
    >
      <div className="px-4 pb-4 font-mono text-sm leading-relaxed">
        <p className="text-syntax-comment">{header(entry)}</p>
        <pre className="whitespace-pre-wrap break-words text-fg">{sql}</pre>
        {entry.error && (
          <pre className="whitespace-pre-wrap break-words text-danger">
            {entry.error
              .split("\n")
              .map((line) => `-- ${line}`)
              .join("\n")}
          </pre>
        )}
      </div>
    </ContextMenuArea>
  );
});

/**
 * The statement as spans in the editor's token colors. The classes come from the editor's
 * highlight style, which every tab's editor keeps mounted in the document.
 */
function highlighted(sql: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const tree = MySQL.language.parser.parse(sql);
  highlightCode(
    sql,
    tree,
    sqlHighlightStyle,
    (text, classes) => {
      const key = nodes.length;
      nodes.push(
        classes ? (
          <span key={key} className={classes}>
            {text}
          </span>
        ) : (
          text
        ),
      );
    },
    () => nodes.push("\n"),
  );
  return nodes;
}

/** `21:10:24.272`, local time. */
function clock(at: string): string {
  const d = new Date(at);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}.${String(d.getMilliseconds()).padStart(3, "0")}`;
}

function ms(value: number): string {
  if (value < 1) return `${value.toFixed(2)} ms`;
  if (value < 1000) return `${Math.round(value)} ms`;
  return `${(value / 1000).toFixed(2)} s`;
}

function count(n: number, word: string): string {
  return `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;
}

/** `-- 21:10:24.272 · 1,000 rows · 13 ms, fetch 15 ms`; a failed statement says so. */
function header(entry: LogEntry): string {
  const parts = [clock(entry.at)];
  const source = SOURCE_LABEL[entry.source];
  if (source) parts.push(source);
  if (entry.error) {
    parts.push("failed");
  } else {
    if (entry.rows !== null) {
      parts.push(`${count(entry.rows, "row")}${entry.truncated ? " (more not kept)" : ""}`);
    }
    if (entry.affected !== null) parts.push(`${count(entry.affected, "row")} affected`);
  }
  parts.push(
    entry.fetch_ms === null
      ? ms(entry.execute_ms)
      : `${ms(entry.execute_ms)}, fetch ${ms(entry.fetch_ms)}`,
  );
  return `-- ${parts.join(" · ")}`;
}

function terminated(sql: string): string {
  return sql.endsWith(";") || sql.endsWith("…") ? sql : `${sql};`;
}

function entryText(entry: LogEntry): string {
  const error = entry.error ? `\n${entry.error.replace(/^/gm, "-- ")}` : "";
  return `${header(entry)}\n${terminated(entry.sql)}${error}`;
}
