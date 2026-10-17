import type { ButtonHTMLAttributes } from "react";
import { cx } from "./cx";

/**
 * Secondary control: an engraved plate with a hairline edge. It never uses
 * `signal`; that colour belongs to the single primary signal control.
 */
export function Button({
  className,
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      type={type}
      className={cx(
        "plate-lg inline-flex h-11 cursor-pointer items-center gap-2 rounded-plate border border-hairline-lit px-4 text-silk-dim",
        "transition-colors duration-[160ms] ease-settle hover:border-engrave hover:text-silk",
        "disabled:cursor-not-allowed disabled:border-hairline disabled:text-engrave",
        className,
      )}
      {...props}
    />
  );
}
