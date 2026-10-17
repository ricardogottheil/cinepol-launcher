import type { ReactNode } from "react";
import { cx } from "./cx";

export interface ReadoutProps {
  /** The unit plate above the value, e.g. "Versions". */
  unit: string;
  /** The measured value. `null` or `undefined` renders an em dash, never 0. */
  value?: string | null;
  size?: "xl" | "md" | "sm";
  /** Rendered before the unit, e.g. a status lamp. */
  lead?: ReactNode;
  className?: string;
}

const VALUE_SIZE = { xl: "readout-xl", md: "readout-md", sm: "readout-sm" } as const;

/** Unit above value, every number in the readout face (Readout.astro). */
export function Readout({ unit, value, size = "md", lead, className }: ReadoutProps) {
  const known = value !== null && value !== undefined && value !== "";
  return (
    <div className={cx("flex flex-col gap-1", className)}>
      <span className="plate flex items-center gap-2 text-engrave">
        {lead}
        {unit}
      </span>
      <span data-readout className={cx(VALUE_SIZE[size], known ? "text-silk" : "text-engrave")}>
        {known ? value : "—"}
      </span>
    </div>
  );
}
