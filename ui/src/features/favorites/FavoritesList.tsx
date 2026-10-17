import { FileCode2, Star, Table2 } from "lucide-react";
import { ContextMenuArea } from "@/components/ui/Menu";
import { TreeRow } from "@/components/ui/TreeRow";
import { useApp } from "@/store/app";

const ICON = { size: 15, strokeWidth: 1.75 } as const;

/** Pinned tables and saved queries. `query` (the sidebar filter) matches names. */
export function FavoritesList({ query }: { query: string }) {
  const favorites = useApp((s) => s.favorites);
  const connections = useApp((s) => s.connections);
  const { openFavorite, deleteFavorite } = useApp.getState();
  const needle = query.trim().toLowerCase();
  const shown = needle ? favorites.filter((f) => f.name.toLowerCase().includes(needle)) : favorites;

  if (shown.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center gap-2 px-4 pt-16 text-center">
        <Star size={28} strokeWidth={1.5} className="text-faint" />
        <p className="font-semibold text-base text-fg">
          {needle ? "No matching favorites" : "No favorites yet"}
        </p>
        <p className="text-sm text-muted">
          Right-click a table, or use Save query… in a query tab.
        </p>
      </div>
    );
  }

  return (
    <div role="tree" className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
      {shown.map((favorite) => {
        const conn = connections.find((c) => c.id === favorite.conn)?.name ?? "Deleted connection";
        const where = favorite.database ? `${conn} · ${favorite.database}` : conn;
        return (
          <ContextMenuArea
            key={favorite.id}
            items={[
              { label: "Open", onSelect: () => openFavorite(favorite) },
              {
                label: "Remove from favorites",
                onSelect: () => deleteFavorite(favorite.id),
                danger: true,
                separatorBefore: true,
              },
            ]}
          >
            <TreeRow
              depth={0}
              icon={favorite.item.kind === "table" ? <Table2 {...ICON} /> : <FileCode2 {...ICON} />}
              label={favorite.name}
              title={favorite.item.kind === "query" ? favorite.item.sql : where}
              onClick={() => openFavorite(favorite)}
              trailing={<span className="truncate text-faint text-xs">{where}</span>}
            />
          </ContextMenuArea>
        );
      })}
    </div>
  );
}
