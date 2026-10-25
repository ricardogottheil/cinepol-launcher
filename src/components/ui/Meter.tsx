import { cx } from "./cx";

export interface MeterProps {
  /** Accessible name of the measured operation. */
  label: string;
  value: number;
  /** `null` when the total is unknown: the track stays empty, the count still reads. */
  max: number | null;
  /** Renders the count (default: zero-padded integers). */
  format?: (value: number) => string;
  /** Unit label after the count, such as "MB". */
  unitLabel?: string;
  className?: string;
}

/**
 * Progress as an instrument reading: a hairline track filled in signal and
 * the count as a tabular readout.
 */
export function Meter({ label, value, max, format, unitLabel, className }: MeterProps) {
  const ratio = max ? Math.min(1, Math.max(0, value / max)) : 0;
  const width = String(max === null ? 0 : max).length;
  return (
    <div className={cx("flex items-center gap-4", className)}>
      <div
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuenow={value}
        {...(max === null ? {} : { "aria-valuemax": max })}
        className="h-1 flex-1 bg-hairline"
      >
        <div
          className="h-full bg-signal transition-[width] duration-[160ms] ease-settle"
          style={{ width: `${ratio * 100}%` }}
        />
      </div>
      <span data-readout className="readout-sm text-silk-dim">
        {format
          ? `${format(value)} / ${max === null ? "—" : format(max)}`
          : `${String(value).padStart(width, "0")} / ${max ?? "—"}`}
        {unitLabel ? ` ${unitLabel}` : null}
      </span>
    </div>
  );
}
