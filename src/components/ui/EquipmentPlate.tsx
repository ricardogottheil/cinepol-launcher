import { cx } from "./cx";

export interface EquipmentPlateProps {
  /** One to three lines: subject, color temperature, status. */
  lines: string[];
  className?: string;
}

/**
 * A silkscreened plate (DESIGN.md "Equipment plate"): the cinema caption that
 * labels a world image, e.g. "Resource world / 5600 K". It carries its own
 * ground, so type never sits on a bare screenshot.
 */
export function EquipmentPlate({ lines, className }: EquipmentPlateProps) {
  return (
    <div
      className={cx(
        "plate inline-flex flex-col gap-1 rounded-plate border border-hairline-lit bg-panel px-3 py-2",
        className,
      )}
    >
      {lines.map((line, index) => (
        <span key={line} className={index === 0 ? "text-silk" : "text-silk-dim"}>
          {line}
        </span>
      ))}
    </div>
  );
}
