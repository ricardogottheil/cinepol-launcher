import type { ComponentProps } from "react";
import { cx } from "./cx";

/** The secondary plate look for a link that acts like a `Button`. */
export const plateLinkClass =
  "plate-lg inline-flex h-11 items-center rounded-plate border border-hairline-lit px-4 text-silk-dim no-underline transition-colors duration-[160ms] ease-settle hover:border-engrave hover:text-silk";

/**
 * Secondary control: an engraved plate with a hairline edge. It never uses
 * `signal`; that colour belongs to the single primary signal control.
 */
export function Button({ className, type = "button", ...props }: ComponentProps<"button">) {
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
