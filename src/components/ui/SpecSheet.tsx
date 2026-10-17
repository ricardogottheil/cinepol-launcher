import { cx } from "./cx";

export interface SpecRow {
  label: string;
  /** `null` renders an em dash in engrave. */
  value: string | null;
}

/**
 * Label -> value spec sheet (the website's world readings: "Mundo / Se
 * reinicia"). Used for instance details: version, loader, Java, memory.
 */
export function SpecSheet({ rows, className }: { rows: SpecRow[]; className?: string }) {
  return (
    <dl className={cx("m-0 flex flex-col gap-3", className)}>
      {rows.map((row) => (
        <div key={row.label} className="flex flex-col gap-0.5">
          <dt className="plate text-engrave">{row.label}</dt>
          <dd
            data-readout
            className={cx("readout-md m-0", row.value ? "text-silk" : "text-engrave")}
          >
            {row.value ?? "—"}
          </dd>
        </div>
      ))}
    </dl>
  );
}
