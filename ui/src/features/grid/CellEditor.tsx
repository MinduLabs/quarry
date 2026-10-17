import { useEffect, useRef } from "react";

/**
 * Inline text editor over a grid cell. Enter or leaving the cell keeps the text; Escape drops it.
 * The text is sent as is and the database converts it to the column type.
 */
export function CellEditor({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (text: string) => void;
  onCancel: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const done = useRef(false);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const finish = (keep: boolean) => {
    if (done.current) return;
    done.current = true;
    const text = input.current?.value ?? initial;
    if (keep && text !== initial) onCommit(text);
    else onCancel();
  };

  return (
    <input
      ref={input}
      defaultValue={initial}
      spellCheck={false}
      aria-label="Cell value"
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") finish(true);
        else if (e.key === "Escape") finish(false);
      }}
      onBlur={() => finish(true)}
      onMouseDown={(e) => e.stopPropagation()}
      className="absolute inset-0 z-10 bg-app px-2 font-mono text-fg text-sm outline-1 outline-accent -outline-offset-1"
    />
  );
}
