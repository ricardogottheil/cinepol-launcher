import wordmark from "../../assets/brand/logo-wordmark.webp";
import { cx } from "./cx";

/** The CINEPOL wordmark, the only logo rendered (never logo.png). */
export function Wordmark({ className }: { className?: string }) {
  return <img src={wordmark} alt="CINEPOL" className={cx("block h-8 w-auto", className)} />;
}
