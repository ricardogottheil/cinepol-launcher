import { cx } from "./cx";
import { EquipmentPlate } from "./EquipmentPlate";

export type World = "overworld" | "nether" | "end";

/** Color temperature of each world reading (DESIGN.md "The three readings"). */
export const WORLD_TEMPERATURE: Record<World, string> = {
  overworld: "5600 K",
  nether: "2000 K",
  end: "7500 K",
};

export interface WorldFrameProps {
  world: World;
  src: string;
  /** Subject of the capture, shown on the plate. Decorative images pass "". */
  caption: string;
  className?: string;
}

/** A 16:9 server capture with its equipment plate (WorldGallery.astro). */
export function WorldFrame({ world, src, caption, className }: WorldFrameProps) {
  return (
    <figure
      className={cx("relative m-0 overflow-hidden rounded-plate border border-hairline", className)}
    >
      <img src={src} alt="" className="block aspect-video w-full object-cover" />
      <figcaption className="absolute bottom-3 left-3">
        <EquipmentPlate lines={[caption, WORLD_TEMPERATURE[world]]} />
      </figcaption>
    </figure>
  );
}
