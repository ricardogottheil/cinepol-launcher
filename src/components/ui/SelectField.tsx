import { type SelectHTMLAttributes, useId } from "react";
import { cx } from "./cx";

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectFieldProps extends Omit<SelectHTMLAttributes<HTMLSelectElement>, "id"> {
  label: string;
  options: SelectOption[];
}

/** A native select (full keyboard and screen reader support) in the engraved style. */
export function SelectField({ label, options, className, ...props }: SelectFieldProps) {
  const id = useId();
  return (
    <div className={cx("flex flex-col gap-2", className)}>
      <label htmlFor={id} className="plate text-engrave">
        {label}
      </label>
      <select
        id={id}
        className="readout-sm h-11 rounded-plate border border-hairline-lit bg-panel-deep px-3 text-silk"
        {...props}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </div>
  );
}
