import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** Accessible name; icon buttons have no visible text. */
  label: string;
  children: ReactNode;
}

/** A 44px square plate holding one drawn glyph (BackToTop / Lightbox controls). */
export function IconButton({
  label,
  className,
  type = "button",
  children,
  ...props
}: IconButtonProps) {
  return (
    <button
      type={type}
      aria-label={label}
      title={label}
      className={cx(
        "inline-flex size-11 cursor-pointer items-center justify-center rounded-plate border border-hairline text-silk-dim",
        "transition-colors duration-[160ms] ease-settle hover:border-hairline-lit hover:text-silk",
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}
