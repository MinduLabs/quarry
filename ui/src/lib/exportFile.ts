/** Characters a file name cannot hold on at least one of the systems Quarry runs on. */
const UNSAFE_IN_FILE_NAME = /[\\/:*?"<>|]/g;

/** The text without control characters (a line break in a table name would reach the dialog). */
function withoutControlCharacters(text: string): string {
  return Array.from(text)
    .filter((ch) => (ch.codePointAt(0) ?? 0) >= 0x20)
    .join("");
}

/** The name an export is suggested to be saved as: the table or result it holds, then the
 * format's extension. A name with nothing left to use falls back to `result`. */
export function suggestedFileName(source: string | null, extension: string): string {
  const base = withoutControlCharacters(source ?? "")
    .replace(UNSAFE_IN_FILE_NAME, "_")
    .trim()
    .replace(/^\.+/, "");
  return `${base === "" ? "result" : base}.${extension}`;
}
