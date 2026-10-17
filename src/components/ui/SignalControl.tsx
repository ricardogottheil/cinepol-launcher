import { useEffect, useRef, useState } from "react";
import { cx } from "./cx";

const FLASH_MS = 420;

export interface SignalControlProps {
  /** Action label, set in the control voice (uppercase). */
  label: string;
  /** Optional readout shown before the label, e.g. the server address. */
  detail?: string;
  onPress?: () => void;
  size?: "full" | "compact";
  className?: string;
}

/**
 * The one control that carries `signal` green. Ported from the website's
 * copy-IP control (CopyAddress.astro): hard-edged, dark ink on green, and it
 * acknowledges a press with the shutter flash.
 */
export function SignalControl({
  label,
  detail,
  onPress,
  size = "full",
  className,
}: SignalControlProps) {
  const [pressed, setPressed] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  function handleClick() {
    setPressed(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setPressed(false), FLASH_MS);
    onPress?.();
  }

  return (
    <button
      type="button"
      data-signal-control
      data-state={pressed ? "pressed" : "idle"}
      onClick={handleClick}
      className={cx(
        "inline-flex cursor-pointer items-center rounded-control bg-signal text-on-signal",
        "transition-[background-color,transform] duration-[120ms] ease-out hover:bg-signal-lit active:translate-y-px",
        size === "full" ? "h-14 w-full px-5" : "h-11 px-3.5",
        className,
      )}
    >
      <span className="flex w-full items-baseline justify-between gap-3">
        {detail && size === "full" ? (
          <span data-readout className="readout-md truncate">
            {detail}
          </span>
        ) : null}
        <span className="control-label">{label}</span>
      </span>
    </button>
  );
}
