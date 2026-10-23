import { useEffect, useRef, useState } from "react";
import { cx } from "./cx";

export interface PlayerHeadProps {
  /** Data URL of the skin PNG. `null` shows the name's initial instead. */
  skin: string | null;
  name: string;
  size?: "sm" | "lg";
  className?: string;
}

const SIZES = {
  sm: "h-9 w-9 text-[0.875rem]",
  lg: "h-16 w-16 text-[1.5rem]",
} as const;

/**
 * The front of a player's head, read from the skin texture: the face (8,8)
 * with its hat layer (40,8) on top, scaled by whole pixels. Profiles without a
 * skin get their initial on a plate, never one of the game's default players.
 * Decorative: the name always sits next to it.
 */
export function PlayerHead({ skin, name, size = "sm", className }: PlayerHeadProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [drawn, setDrawn] = useState(false);

  useEffect(() => {
    setDrawn(false);
    const context = canvas.current?.getContext("2d");
    if (!skin || !context) return;
    let current = true;
    const image = new Image();
    image.onload = () => {
      if (!current) return;
      context.imageSmoothingEnabled = false;
      context.clearRect(0, 0, 8, 8);
      context.drawImage(image, 8, 8, 8, 8, 0, 0, 8, 8);
      context.drawImage(image, 40, 8, 8, 8, 0, 0, 8, 8);
      setDrawn(true);
    };
    image.src = skin;
    return () => {
      current = false;
    };
  }, [skin]);

  return (
    <span
      aria-hidden="true"
      className={cx(
        "relative inline-grid shrink-0 place-items-center overflow-hidden rounded-plate border border-hairline-lit bg-panel-deep font-display font-semibold text-silk-dim",
        SIZES[size],
        className,
      )}
    >
      <canvas
        ref={canvas}
        width={8}
        height={8}
        className={cx(
          "absolute inset-0 h-full w-full [image-rendering:pixelated]",
          !drawn && "invisible",
        )}
      />
      {drawn ? null : name.slice(0, 1).toUpperCase()}
    </span>
  );
}
