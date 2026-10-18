import { type TextareaHTMLAttributes, useId } from "react";
import { cx } from "./cx";

export interface TextAreaFieldProps
  extends Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "id"> {
  label: string;
  hint?: string;
  /** Shown under the field in place of the hint; marks the field invalid. */
  error?: string | undefined;
}

/** A labelled multi-line input in the readout face, styled like `TextField`. */
export function TextAreaField({ label, hint, error, className, ...props }: TextAreaFieldProps) {
  const id = useId();
  const described = `${id}-description`;
  return (
    <div className={cx("flex flex-col gap-2", className)}>
      <label htmlFor={id} className="plate text-engrave">
        {label}
      </label>
      <textarea
        id={id}
        rows={3}
        spellCheck={false}
        aria-invalid={error ? true : undefined}
        aria-describedby={hint || error ? described : undefined}
        className={cx(
          "readout-sm min-h-24 rounded-plate border bg-panel-deep px-3 py-2 text-silk placeholder:text-engrave",
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
