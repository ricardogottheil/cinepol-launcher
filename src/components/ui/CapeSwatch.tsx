import { useEffect, useRef, useState } from "react";
import { cx } from "./cx";

/**
 * The front of a cape (10x16 pixels at 1,1 of its texture), pixel-sharp.
 * Without a texture it stays an empty plate. Decorative: the cape's name
 * always sits next to it.
 */
export function CapeSwatch({ texture, className }: { texture: string | null; className?: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [drawn, setDrawn] = useState(false);

  useEffect(() => {
    setDrawn(false);
    const context = canvas.current?.getContext("2d");
    if (!texture || !context) return;
    let current = true;
    const image = new Image();
    image.onload = () => {
      if (!current) return;
      context.imageSmoothingEnabled = false;
      context.clearRect(0, 0, 10, 16);
      context.drawImage(image, 1, 1, 10, 16, 0, 0, 10, 16);
      setDrawn(true);
    };
    image.src = texture;
    return () => {
      current = false;
    };
  }, [texture]);

  return (
    <span
      aria-hidden="true"
      className={cx(
        "relative block h-16 w-10 shrink-0 overflow-hidden rounded-plate border border-hairline-lit bg-panel-deep",
        className,
      )}
    >
      <canvas
        ref={canvas}
        width={10}
        height={16}
        className={cx(
          "absolute inset-0 h-full w-full [image-rendering:pixelated]",
          !drawn && "invisible",
        )}
      />
    </span>
  );
}
