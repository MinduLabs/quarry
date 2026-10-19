import { completeFromList, snippetCompletion } from "@codemirror/autocomplete";

/**
 * Built-in templates. Typing the label and pressing Enter or Tab inserts the template; Tab then
 * moves between the `#{…}` fields. They sit in the normal completion list, so they need no extra
 * shortcut. Fields use `#{}` because CodeMirror reads it like `${}` and Biome flags `${` in
 * plain strings.
 */
const TEMPLATES: { label: string; detail: string; template: string }[] = [
  {
    label: "sel",
    detail: "SELECT … FROM",
    template: "SELECT #{*}\nFROM #{table}\nWHERE #{condition};",
  },
  {
    label: "selc",
    detail: "SELECT COUNT(*)",
    template: "SELECT COUNT(*)\nFROM #{table}\nWHERE #{condition};",
  },
  {
    label: "selg",
    detail: "SELECT … GROUP BY",
    template:
      "SELECT #{column}, COUNT(*) AS total\nFROM #{table}\nGROUP BY #{column}\nORDER BY total DESC;",
  },
  {
    label: "selj",
    detail: "SELECT … JOIN",
    template:
      "SELECT #{a}.#{*}\nFROM #{table} AS #{a}\nJOIN #{other} AS #{b} ON #{b}.#{id} = #{a}.#{id}\nWHERE #{condition};",
  },
  {
    label: "ins",
    detail: "INSERT INTO … VALUES",
    template: "INSERT INTO #{table} (#{columns})\nVALUES (#{values});",
  },
  {
    label: "upd",
    detail: "UPDATE … SET … WHERE",
    template: "UPDATE #{table}\nSET #{column} = #{value}\nWHERE #{condition};",
  },
  {
    label: "del",
    detail: "DELETE FROM … WHERE",
    template: "DELETE FROM #{table}\nWHERE #{condition};",
  },
  {
    label: "ct",
    detail: "CREATE TABLE",
    template:
      "CREATE TABLE #{name} (\n  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,\n  #{column} VARCHAR(255) NOT NULL,\n  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,\n  PRIMARY KEY (id)\n);",
  },
  {
    label: "ci",
    detail: "CREATE INDEX",
    template: "CREATE INDEX #{name} ON #{table} (#{columns});",
  },
  {
    label: "cte",
    detail: "WITH … AS",
    template: "WITH #{name} AS (\n  #{query}\n)\nSELECT #{*}\nFROM #{name};",
  },
  {
    label: "dups",
    detail: "Find duplicate values",
    template:
      "SELECT #{column}, COUNT(*) AS total\nFROM #{table}\nGROUP BY #{column}\nHAVING total > 1;",
  },
];

/** A completion source for the editor's language data. */
export const snippetSource = completeFromList(
  TEMPLATES.map((t) =>
    snippetCompletion(t.template, { label: t.label, detail: t.detail, type: "text", boost: -1 }),
  ),
);
