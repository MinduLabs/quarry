import { useMemo, useState } from "react";
import type { ColumnMeta } from "@/bindings/ColumnMeta";
import type { Value } from "@/bindings/Value";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { copyValue, formatHex, hexToText, parseHex, textToHex } from "@/lib/values";
import { copyText } from "@/store/app";

type Mode = "text" | "hex";

/**
 * The full value of one cell: long text, JSON (formatted on request) or bytes (as hex or as
 * UTF-8 text). With `editable`, Save puts the new value in the grid's pending edits.
 */
export function ValueDialog({
  column,
  value,
  editable,
  onSave,
  onClose,
}: {
  column: ColumnMeta;
  value: Value | undefined;
  editable: boolean;
  onSave: (value: Value) => void;
  onClose: () => void;
}) {
  const isBytes = value?.t === "bytes";
  const isJson = value?.t === "json" || /json/i.test(column.type_name);
  const decoded = useMemo(() => (value?.t === "bytes" ? hexToText(value.v) : null), [value]);
  // Bytes that are valid text open as text; anything else as hex.
  const [mode, setMode] = useState<Mode>(isBytes && decoded === null ? "hex" : "text");
  const initial = (m: Mode) => {
    if (value?.t === "bytes") return m === "hex" ? formatHex(value.v) : (decoded ?? "");
    return value === undefined || value.t === "null" ? "" : copyValue(value);
  };
  const [text, setText] = useState(() => initial(mode));
  const [isNull, setIsNull] = useState(value === undefined || value.t === "null");

  const jsonError = useMemo(() => {
    if (!isJson || isNull || !text.trim()) return null;
    try {
      JSON.parse(text);
      return null;
    } catch (err) {
      return err instanceof Error ? err.message : "Invalid JSON";
    }
  }, [isJson, isNull, text]);
  const hex = mode === "hex" ? parseHex(text) : null;
  const hexError = isBytes && mode === "hex" && !isNull && hex === null;

  /** The edited value, or null when it can't be saved as typed. */
  const edited = (): Value | null => {
    if (isNull) return { t: "null" };
    if (isBytes) {
      const bytes = mode === "hex" ? hex : textToHex(text);
      return bytes === null ? null : { t: "bytes", v: bytes };
    }
    return { t: "text", v: text };
  };

  const unchanged = (next: Value) => {
    const wasNull = value === undefined || value.t === "null";
    if (next.t === "null" || wasNull) return next.t === "null" && wasNull;
    if (next.t === "bytes") return value.t === "bytes" && value.v === next.v;
    return next.t === "text" && next.v === copyValue(value);
  };

  const switchMode = (next: Mode) => {
    if (next === mode) return;
    // Carry what was typed over to the other view.
    const bytes = mode === "hex" ? parseHex(text) : textToHex(text);
    if (bytes !== null) setText(next === "hex" ? formatHex(bytes) : (hexToText(bytes) ?? ""));
    setMode(next);
  };

  const length = isBytes
    ? `${((mode === "hex" ? (hex ?? "") : textToHex(text)).length / 2).toLocaleString()} bytes`
    : `${text.length.toLocaleString()} characters`;

  return (
    <Dialog
      open
      onClose={onClose}
      title={column.name}
      description={`${column.type_name} · ${isNull ? "NULL" : length}`}
      width={680}
      footer={
        <>
          <Button
            variant="ghost"
            className="mr-auto"
            disabled={isNull}
            onClick={() => copyText(text, "Copied")}
          >
            Copy
          </Button>
          {editable && (
            <Button disabled={isNull} onClick={() => setIsNull(true)}>
              Set NULL
            </Button>
          )}
          <Button onClick={onClose}>{editable ? "Cancel" : "Close"}</Button>
          {editable && (
            <Button
              variant="primary"
              disabled={edited() === null}
              onClick={() => {
                const next = edited();
                if (!next) return;
                if (unchanged(next)) onClose();
                else onSave(next);
              }}
            >
              Save
            </Button>
          )}
        </>
      }
    >
      <div className="flex flex-col gap-2 pb-2">
        {(isBytes || isJson) && (
          <div className="flex items-center gap-2">
            {isBytes && (
              <SegmentedTabs
                label="Show as"
                value={mode}
                onChange={switchMode}
                className="w-40"
                tabs={[
                  { value: "text", label: "Text" },
                  { value: "hex", label: "Hex" },
                ]}
              />
            )}
            {isJson && (
              <Button
                size="sm"
                disabled={isNull || jsonError !== null || !text.trim()}
                onClick={() => setText(JSON.stringify(JSON.parse(text), null, 2))}
              >
                Format JSON
              </Button>
            )}
          </div>
        )}
        <textarea
          aria-label="Value"
          spellCheck={false}
          readOnly={!editable}
          value={isNull ? "" : text}
          placeholder={isNull ? "NULL" : undefined}
          onChange={(e) => {
            setIsNull(false);
            setText(e.target.value);
          }}
          className="h-80 w-full resize-y rounded-md border border-line-strong bg-app p-2 font-mono text-fg text-sm placeholder:text-faint placeholder:italic focus:border-accent focus:outline-none focus:ring-3 focus:ring-accent-soft"
        />
        {jsonError && <p className="text-sm text-warning">Not valid JSON: {jsonError}</p>}
        {hexError && (
          <p className="text-sm text-danger">Hex needs pairs of 0–9 and A–F, e.g. DE AD BE EF.</p>
        )}
        {isBytes && mode === "text" && decoded === null && !isNull && (
          <p className="text-sm text-warning">
            These bytes are not valid UTF-8 text; saving from this view replaces them.
          </p>
        )}
      </div>
    </Dialog>
  );
}
