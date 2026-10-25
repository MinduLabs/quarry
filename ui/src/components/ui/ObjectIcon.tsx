import {
  Braces,
  CalendarClock,
  Eye,
  Hash,
  KeyRound,
  Link2,
  SquareTerminal,
  Table2,
  Zap,
} from "lucide-react";
import { cn } from "@/lib/cn";

/** What an icon in the sidebar stands for: a kind of schema object, or a part of a table. */
export type ObjectIconKind =
  | "table"
  | "view"
  | "procedure"
  | "function"
  | "trigger"
  | "event"
  | "index"
  | "primary_key"
  | "foreign_key";

const ICONS = {
  table: [Table2, "text-icon-table"],
  view: [Eye, "text-icon-view"],
  procedure: [SquareTerminal, "text-icon-procedure"],
  function: [Braces, "text-icon-function"],
  trigger: [Zap, "text-icon-trigger"],
  event: [CalendarClock, "text-icon-trigger"],
  index: [Hash, "text-icon-index"],
  primary_key: [KeyRound, "text-icon-key"],
  foreign_key: [Link2, "text-icon-link"],
} as const;

/** The `ObjectKind` of the engine (`materialized_view`, …) as an icon kind; unknown ones read as functions. */
export function iconKindOf(kind: string): ObjectIconKind {
  if (kind === "materialized_view") return "view";
  return kind in ICONS ? (kind as ObjectIconKind) : "function";
}

/** A schema object's icon, in the color of its kind (tokens `--icon-*`). */
export function ObjectIcon({
  kind,
  size = 15,
  className,
}: {
  kind: ObjectIconKind;
  size?: number;
  className?: string;
}) {
  const [Icon, color] = ICONS[kind];
  return <Icon size={size} strokeWidth={1.75} className={cn(color, className)} aria-hidden />;
}
