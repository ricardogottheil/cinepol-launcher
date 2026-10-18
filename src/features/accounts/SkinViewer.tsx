import { useEffect, useRef } from "react";
import { cx } from "../../components/ui/cx";

export interface SkinViewerProps {
  /** Data URL or blob URL of the skin PNG. Remote URLs are never fetched here. */
  texture: string | null;
  slim: boolean;
  /** Accessible name of the preview. */
  label: string;
  className?: string;
}

// The look of the website's hero figure (cinepol-web src/scripts/hero-model.ts):
// a fixed front-facing camera, a high flat fill plus one weak key, and only the
// raised hand moving: three swings, a held beat, again.
const FOV_DEG = 28;
const HEMISPHERE_INTENSITY = 2.1;
const KEY_INTENSITY = 0.85;
const WHITE_LIGHT = 0xffffff;
/** Arm out and up from the shoulder; the website measured 100 degrees. */
const ARM_RAISED = (100 * Math.PI) / 180;
/** Half-swing of the wave: the swing spans 89 to 111 degrees. */
const WAVE_SWING = (11 * Math.PI) / 180;
const WAVE_CYCLES = 3;
const WAVE_MS = 1180;
const HOLD_MS = 1600;
const TAU = Math.PI * 2;
/**
 * With the left arm raised, the posed figure spans from the right arm's outer
 * face (x = -8) to the raised hand (about x = 18). Shifting it left centers that
 * box on the camera.
 */
const FIGURE_SHIFT_X = -5;
const LOOP_THRESHOLD = 0.05;

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
    : false;
}

/** Arm angle at a point of the wave-and-hold loop; `seconds` is animation time. */
function waveAngle(seconds: number): number {
  const cycleMs = (seconds * 1000) % (WAVE_MS + HOLD_MS);
  // Whole sine cycles, so each swing starts and ends at the raised pose.
  const t = Math.min(1, cycleMs / WAVE_MS);
  return ARM_RAISED + Math.sin(t * WAVE_CYCLES * TAU) * WAVE_SWING;
}

/** A 3D preview of a Minecraft skin, framed like the website's skin section. */
export function SkinViewer({ texture, slim, label, className }: SkinViewerProps) {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || texture === null) return;

    let cancelled = false;
    let cleanup: (() => void) | null = null;

    // Loaded on demand so skinview3d and three stay out of the main bundle.
    void Promise.all([import("skinview3d"), import("three")])
      .then(([skinview3d, THREE]) => {
        if (cancelled) return;

        const viewer = new skinview3d.SkinViewer({
          width: Math.max(1, host.clientWidth),
          height: Math.max(1, host.clientHeight),
          skin: texture,
          model: slim ? "slim" : "default",
          fov: FOV_DEG,
          enableControls: false,
          pixelRatio: "match-device",
        });

        const canvas = viewer.canvas;
        canvas.setAttribute("aria-hidden", "true");
        canvas.className = "absolute inset-0 block size-full";
        host.append(canvas);

        // Replace skinview3d's ambient and camera lights with the website's rig.
        viewer.globalLight.intensity = 0;
        viewer.cameraLight.intensity = 0;
        const panel = getComputedStyle(host).getPropertyValue("--color-panel").trim();
        viewer.scene.add(
          new THREE.HemisphereLight(WHITE_LIGHT, panel || WHITE_LIGHT, HEMISPHERE_INTENSITY),
        );
        const key = new THREE.DirectionalLight(WHITE_LIGHT, KEY_INTENSITY);
        key.position.set(-4, 6, 5);
        viewer.scene.add(key);

        viewer.playerObject.position.x = FIGURE_SHIFT_X;
        const arm = viewer.playerObject.skin.leftArm;
        if (prefersReducedMotion()) {
          // Nothing moves, and the figure still greets with the raised hand.
          arm.rotation.z = ARM_RAISED;
        } else {
          viewer.animation = new skinview3d.FunctionAnimation((player, progress) => {
            player.skin.leftArm.rotation.z = waveAngle(progress);
          });
        }

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
          viewer.dispose();
          // dispose() keeps the context alive until garbage collection; release
          // it now so swapping skins never piles up WebGL contexts.
          viewer.renderer.forceContextLoss();
          canvas.remove();
        };
      })
      .catch(() => {
        // No WebGL or the chunk failed to load: the empty frame stays.
      });

    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [texture, slim]);

  return (
    <div
      ref={hostRef}
      role="img"
      aria-label={label}
      className={cx(
        "relative aspect-3/4 w-full overflow-hidden rounded-plate border border-hairline bg-panel",
        className,
      )}
    />
  );
}
