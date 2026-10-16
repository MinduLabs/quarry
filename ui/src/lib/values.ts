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
