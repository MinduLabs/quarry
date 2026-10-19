import type { Value } from "@/bindings/Value";

const MAX_CHARS = 200;

/** Single-line text for a grid cell. */
export function displayValue(value: Value | undefined): string {
  if (!value) return "";
  switch (value.t) {
    case "null":
      return "NULL";
    case "bool":
      return value.v ? "true" : "false";
    case "float":
      return String(value.v);
    case "bytes":
      return value.v.length > 32
        ? `0x${value.v.slice(0, 32).toUpperCase()}… (${value.v.length / 2} bytes)`
        : `0x${value.v.toUpperCase()}`;
    case "array":
      return `[${value.v.map(displayValue).join(", ")}]`;
    case "other":
      return oneLine(value.v.text);
    case "timestamp":
    case "timestamp_tz":
      return value.v.replace("T", " ");
    default:
      return oneLine(String(value.v));
  }
}

/** Full text for copying. */
export function copyValue(value: Value | undefined): string {
  if (!value || value.t === "null") return "";
  if (value.t === "other") return value.v.text;
  if (value.t === "array") return JSON.stringify(value.v.map(copyValue));
  return String(value.v);
}

/** Values that don't fit a one-line editor: JSON, bytes, several lines or long text. */
export function isLongValue(value: Value | undefined): boolean {
  if (!value) return false;
  if (value.t === "json" || value.t === "bytes") return true;
  const text = copyValue(value);
  return text.length > MAX_CHARS || /[\r\n]/.test(text);
}

/** UTF-8 text of hex-encoded bytes, or null when they are not valid UTF-8. */
export function hexToText(hex: string): string | null {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++)
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/** Lowercase hex of the UTF-8 bytes of `text`. */
export function textToHex(text: string): string {
  return Array.from(new TextEncoder().encode(text), (b) => b.toString(16).padStart(2, "0")).join(
    "",
  );
}

/** Hex as typed (spaces, `0x`, any case) normalized to lowercase pairs, or null if invalid. */
export function parseHex(typed: string): string | null {
  const hex = typed.replace(/^0x/i, "").replace(/\s+/g, "").toLowerCase();
  return /^([0-9a-f]{2})*$/.test(hex) ? hex : null;
}

/** Hex in groups of two bytes, 16 bytes per line, for reading. */
export function formatHex(hex: string): string {
  const lines: string[] = [];
  for (let i = 0; i < hex.length; i += 32) {
    lines.push(
      hex
        .slice(i, i + 32)
        .replace(/(.{4})(?=.)/g, "$1 ")
        .toUpperCase(),
    );
  }
  return lines.join("\n");
}

function oneLine(text: string): string {
  const flat = text.replace(/[\r\n\t]+/g, " ");
  return flat.length > MAX_CHARS ? `${flat.slice(0, MAX_CHARS)}…` : flat;
}

export function isNumericType(typeName: string): boolean {
  return /INT|DECIMAL|NUMERIC|FLOAT|DOUBLE|REAL/i.test(typeName);
}

export function quoteIdent(name: string): string {
  return `\`${name.replaceAll("`", "``")}\``;
}
