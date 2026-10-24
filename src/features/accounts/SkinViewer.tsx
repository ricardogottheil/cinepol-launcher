import { type PointerEvent, useEffect, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { SkinViewer as Viewer } from "skinview3d";
import { cx } from "../../components/ui/cx";
import { IconButton } from "../../components/ui/IconButton";

export interface SkinViewerProps {
  /** Data URL or blob URL of the skin PNG. Remote URLs are never fetched here. */
  texture: string | null;
  slim: boolean;
  /** Data URL of the cape the account wears, or null for none. */
  cape?: string | null;
  /** Accessible name of the preview. */
  label: string;
  className?: string;
}

// The website's hero figure (cinepol-web src/scripts/hero-model.ts) is lit by a
// high fill plus one key. Its numbers come out muddy here: skinview3d turns off
// three's color management and draws through an FXAA pass with no sRGB output,
// so nothing brightens the midtones. These are set so the side facing the
// player shows the skin's own colors, the top a little brighter and the
// underside half as bright. The figure turns, so the key rides on the camera.
const HEMISPHERE_INTENSITY = 2.4;
const KEY_INTENSITY = 1.6;
/**
 * Above and to the left of the camera, in camera space; it aims at the figure.
 * At skinview3d's default framing the camera stands about 44 units away.
 */
const KEY_OFFSET = [-17, 22, 0] as const;
const WHITE_LIGHT = 0xffffff;
/** Half the sky light, so the underside still reads. */
const GROUND_LIGHT = 0x808080;
/** Front three-quarter view: the face reads, and so does the cape behind it. */
const THREE_QUARTER = -Math.PI / 5;
/** One press of a turn button. */
const TURN_STEP = Math.PI / 4;
/** How fast a button turn closes the gap to its target, per second. */
const TURN_EASE = 9;
/** Radians the figure turns per pixel dragged. */
const DRAG_SPEED = 0.01;
/** The idle pose of skinview3d's IdleAnimation: arms just off the body, cape out. */
const ARM_REST = Math.PI * 0.02;
const ARM_SWAY = 0.03;
const CAPE_REST = Math.PI * 0.06;
const CAPE_SWAY = 0.01;
const LOOP_THRESHOLD = 0.05;

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
    : false;
}

function loadCapeImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("the cape image did not load"));
    image.src = src;
  });
}

/** Arms and cape at a point of the breathing loop; `seconds` is animation time. */
function pose(player: Viewer["playerObject"], seconds: number) {
  const t = seconds * 2;
  player.skin.leftArm.rotation.z = Math.cos(t) * ARM_SWAY + ARM_REST;
  player.skin.rightArm.rotation.z = Math.cos(t + Math.PI) * ARM_SWAY - ARM_REST;
  player.cape.rotation.x = Math.sin(t) * CAPE_SWAY + CAPE_REST;
}

interface Turnable {
  /** Turns the figure by `radians` at once, as a drag does. */
  drag: (radians: number) => void;
  /** Turns the figure by `radians`, eased when motion is allowed. */
  turn: (radians: number) => void;
  viewer: Viewer;
}

/**
 * A 3D preview of a Minecraft skin and cape, lit like the website's hero
 * figure. It can be dragged around or turned with the buttons below it.
 */
export function SkinViewer({ texture, slim, cape = null, label, className }: SkinViewerProps) {
  const { t } = useTranslation();
  const hostRef = useRef<HTMLDivElement>(null);
  const turnableRef = useRef<Turnable | null>(null);
  // Where the pointer was at the last move of a drag; null when not dragging.
  const dragXRef = useRef<number | null>(null);
  // Read when a viewer is built; later changes go through loadCape.
  const capeRef = useRef(cape);
  capeRef.current = cape;
  const hint = useId();

  useEffect(() => {
    const host = hostRef.current;
    if (!host || texture === null) return;

    let cancelled = false;
    let cleanup: (() => void) | null = null;

    // Loaded on demand so skinview3d and three stay out of the main bundle.
    void Promise.all([import("skinview3d"), import("three")])
      .then(([skinview3d, THREE]) => {
        if (cancelled) return;

        const initialCape = capeRef.current;
        const viewer = new skinview3d.SkinViewer({
          width: Math.max(1, host.clientWidth),
          height: Math.max(1, host.clientHeight),
          skin: texture,
          model: slim ? "slim" : "default",
          ...(initialCape ? { cape: initialCape } : {}),
          // The camera never moves; dragging turns the figure (see onPointerMove).
          enableControls: false,
          pixelRatio: "match-device",
        });

        const canvas = viewer.canvas;
        // Released on every path, even if the setup below throws.
        const release = () => {
          turnableRef.current = null;
          viewer.dispose();
          // dispose() keeps the context alive until garbage collection; release
          // it now so swapping skins never piles up WebGL contexts.
          viewer.renderer.forceContextLoss();
          canvas.remove();
        };
        cleanup = release;
        canvas.setAttribute("aria-hidden", "true");
        canvas.className = "absolute inset-0 block size-full cursor-grab active:cursor-grabbing";
        host.append(canvas);

        // Replace skinview3d's ambient and camera lights with the website's rig.
        viewer.globalLight.intensity = 0;
        viewer.cameraLight.intensity = 0;
        viewer.scene.add(
          new THREE.HemisphereLight(WHITE_LIGHT, GROUND_LIGHT, HEMISPHERE_INTENSITY),
        );
        const key = new THREE.DirectionalLight(WHITE_LIGHT, KEY_INTENSITY);
        key.position.set(...KEY_OFFSET);
        viewer.camera.add(key);

        const wrapper = viewer.playerWrapper;
        wrapper.rotation.y = THREE_QUARTER;
        let target: number | null = null;
        const still = prefersReducedMotion();
        if (still) {
          // Nothing moves on its own; button turns land at once.
          pose(viewer.playerObject, 0);
        } else {
          viewer.animation = new skinview3d.FunctionAnimation((player, progress, delta) => {
            pose(player, progress);
            if (target === null) return;
            const gap = target - wrapper.rotation.y;
            if (Math.abs(gap) < 0.001) {
              wrapper.rotation.y = target;
              target = null;
            } else {
              wrapper.rotation.y += gap * Math.min(1, delta * TURN_EASE);
            }
          });
        }

        turnableRef.current = {
          viewer,
          drag: (radians) => {
            target = null;
            wrapper.rotation.y += radians;
          },
          turn: (radians) => {
            if (still) {
              wrapper.rotation.y += radians;
            } else {
              target = (target ?? wrapper.rotation.y) + radians;
            }
          },
        };

        const sizing =
          typeof ResizeObserver === "function"
            ? new ResizeObserver(() => {
                const { clientWidth, clientHeight } = host;
                if (clientWidth > 0 && clientHeight > 0) viewer.setSize(clientWidth, clientHeight);
              })
            : null;
        sizing?.observe(host);

        // Off screen, stop rendering and animating altogether.
        const visibility =
          typeof IntersectionObserver === "function"
            ? new IntersectionObserver(
                (entries) => {
                  for (const entry of entries) viewer.renderPaused = !entry.isIntersecting;
                },
                { threshold: LOOP_THRESHOLD },
              )
            : null;
        visibility?.observe(host);

        cleanup = () => {
          visibility?.disconnect();
          sizing?.disconnect();
          release();
        };
      })
      .catch(() => {
        // No WebGL, the chunk failed to load or the setup threw: release what
        // was created and leave the empty frame.
        cleanup?.();
        cleanup = null;
      });

    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [texture, slim]);

  // A new cape goes onto the figure already on screen, keeping its angle. The
  // image loads here, not in skinview3d, so a cape that arrives after the
  // viewer is gone or after a newer cape is dropped.
  useEffect(() => {
    const viewer = turnableRef.current?.viewer;
    if (!viewer) return;
    if (!cape) {
      viewer.loadCape(null);
      return;
    }
    let current = true;
    const stillWanted = () => current && turnableRef.current?.viewer === viewer;
    loadCapeImage(cape).then(
      (image) => {
        if (stillWanted()) viewer.loadCape(image);
      },
      () => {
        if (stillWanted()) viewer.loadCape(null);
      },
    );
    return () => {
      current = false;
    };
  }, [cape]);

  const turn = (radians: number) => turnableRef.current?.turn(radians);

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    event.currentTarget.setPointerCapture?.(event.pointerId);
    dragXRef.current = event.clientX;
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    if (dragXRef.current === null) return;
    turnableRef.current?.drag((event.clientX - dragXRef.current) * DRAG_SPEED);
    dragXRef.current = event.clientX;
  }

  function onPointerUp(event: PointerEvent<HTMLDivElement>) {
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragXRef.current = null;
  }

  return (
    <figure className={cx("m-0 flex w-full flex-col gap-3", className)}>
      <div
        ref={hostRef}
        role="img"
        aria-label={label}
        aria-describedby={hint}
        className="relative aspect-3/4 w-full touch-none overflow-hidden rounded-plate border border-hairline bg-panel"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      />
      <figcaption className="flex items-center gap-3">
        <IconButton label={t("accounts.turnLeft")} onClick={() => turn(-TURN_STEP)}>
          <TurnGlyph />
        </IconButton>
        <p id={hint} className="plate m-0 flex-1 text-center text-engrave">
          {t("accounts.turnHint")}
        </p>
        <IconButton label={t("accounts.turnRight")} onClick={() => turn(TURN_STEP)}>
          <TurnGlyph mirrored />
        </IconButton>
      </figcaption>
    </figure>
  );
}

/** A curved arrow around the figure's axis; mirrored for the other way. */
function TurnGlyph({ mirrored = false }: { mirrored?: boolean }) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 16 16"
      className={cx("size-4", mirrored && "-scale-x-100")}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
    >
      <path d="M12.5 9.5A5 5 0 1 1 9 3.3" />
      <path d="M9 1v2.6l2.6.4" />
    </svg>
  );
}
