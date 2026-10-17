import { type InputHTMLAttributes, useId } from "react";
import { cx } from "./cx";

export interface TextFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "id"> {
  label: string;
  hint?: string;
  /** Shown under the field in place of the hint; marks the field invalid. */
  error?: string | undefined;
}

/** A labelled input: plate label above, engraved well, hint or error below. */
export function TextField({ label, hint, error, className, ...props }: TextFieldProps) {
  const id = useId();
  const described = `${id}-description`;
  return (
    <div className={cx("flex flex-col gap-2", className)}>
      <label htmlFor={id} className="plate text-engrave">
        {label}
      </label>
      <input
        id={id}
        aria-invalid={error ? true : undefined}
        aria-describedby={hint || error ? described : undefined}
        className={cx(
          "h-11 rounded-plate border bg-panel-deep px-3 text-silk placeholder:text-engrave",
          error ? "border-stale" : "border-hairline-lit",
        )}
        {...props}
      />
      {error || hint ? (
        <p
          id={described}
          className={cx("m-0 text-[0.875rem]", error ? "text-stale" : "text-engrave")}
        >
          {error ?? hint}
        </p>
      ) : null}
    </div>
  );
}
