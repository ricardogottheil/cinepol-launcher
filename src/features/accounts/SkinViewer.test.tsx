import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import es from "../../i18n/locales/es/common.json";
import { SkinViewer } from "./SkinViewer";

interface ViewerOptions {
  skin?: string;
  model?: string;
  cape?: string;
  enableControls?: boolean;
  fov?: number;
}

const fakes = vi.hoisted(() => {
  const viewers: FakeViewer[] = [];
  /** Makes the light setup throw, as a broken WebGL context would. */
  const control = { failSetup: false };

  class FakeFunctionAnimation {
    constructor(
      readonly fn: (player: FakeViewer["playerObject"], progress: number, delta: number) => void,
    ) {}
  }

  class FakeViewer {
    readonly options: ViewerOptions;
    readonly canvas = document.createElement("canvas");
    readonly scene = {
      add: vi.fn(() => {
        if (control.failSetup) throw new Error("setup failed");
      }),
    };
    readonly camera = { add: vi.fn() };
    readonly globalLight = { intensity: 3 };
    readonly cameraLight = { intensity: 0.6 };
    readonly playerObject = {
      skin: { leftArm: { rotation: { z: 0 } }, rightArm: { rotation: { z: 0 } } },
      cape: { rotation: { x: 0 } },
    };
    readonly playerWrapper = { rotation: { y: 0 } };
    readonly controls = {
      enableZoom: true,
      enablePan: false,
      minPolarAngle: 0,
      maxPolarAngle: Math.PI,
      listeners: new Map<string, () => void>(),
      addEventListener(type: string, listener: () => void) {
        this.listeners.set(type, listener);
      },
    };
    autoRotate = false;
    autoRotateSpeed = 1;
    readonly loadCape = vi.fn();
    readonly renderer = { forceContextLoss: vi.fn() };
    animation: unknown = null;
    renderPaused = false;
    readonly setSize = vi.fn();
    readonly dispose = vi.fn();

    constructor(options: ViewerOptions) {
      this.options = options;
      viewers.push(this);
    }
  }

  class FakeLight {
    readonly position = { set: vi.fn() };
  }

  /** Cape images, loaded only when a test says so. */
  const images: FakeImage[] = [];

  class FakeImage {
    src = "";
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;

    constructor() {
      images.push(this);
    }
  }

  return { viewers, control, images, FakeViewer, FakeFunctionAnimation, FakeLight, FakeImage };
});

vi.mock("skinview3d", () => ({
  SkinViewer: fakes.FakeViewer,
  FunctionAnimation: fakes.FakeFunctionAnimation,
}));

vi.mock("three", () => ({
  HemisphereLight: fakes.FakeLight,
  DirectionalLight: fakes.FakeLight,
}));

const TEXTURE = "data:image/png;base64,AAAA";
const OTHER_TEXTURE = "blob:skin-2";
const CAPE = "data:image/png;base64,CAPE";
const OTHER_CAPE = "data:image/png;base64,CAPE2";

function mockReducedMotion(reduced: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      matches: reduced && query.includes("prefers-reduced-motion"),
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
}

/** Finishes loading the cape image for `src`. */
function loadImage(src: string) {
  const image = fakes.images.findLast((candidate) => candidate.src === src);
  if (!image) throw new Error(`no image was requested for ${src}`);
  image.onload?.();
  return image;
}

function lastViewer() {
  const viewer = fakes.viewers.at(-1);
  if (!viewer) throw new Error("no viewer was created");
  return viewer;
}

beforeEach(() => {
  fakes.viewers.length = 0;
  fakes.images.length = 0;
  vi.stubGlobal("Image", fakes.FakeImage);
  mockReducedMotion(false);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("SkinViewer", () => {
  it("exposes the label as the accessible name and hides the canvas", async () => {
    render(<SkinViewer texture={TEXTURE} slim={false} label="Steve's skin" />);

    expect(screen.getByRole("img", { name: "Steve's skin" })).toBeInTheDocument();
    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    expect(lastViewer().canvas).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByRole("img").contains(lastViewer().canvas)).toBe(true);
  });

  it("creates the viewer with the texture and the default model, its camera fixed", async () => {
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();
    expect(viewer.options).toMatchObject({
      skin: TEXTURE,
      model: "default",
      enableControls: false,
    });
    expect(viewer.options.fov).toBeUndefined();
    expect(viewer.options.cape).toBeUndefined();
  });

  it("turns the figure itself when dragged", async () => {
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);
    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();
    const start = viewer.playerWrapper.rotation.y;
    const figure = screen.getByRole("img");

    fireEvent.pointerDown(figure, { clientX: 100, pointerId: 1 });
    fireEvent.pointerMove(figure, { clientX: 150, pointerId: 1 });
    fireEvent.pointerUp(figure, { clientX: 150, pointerId: 1 });
    fireEvent.pointerMove(figure, { clientX: 300, pointerId: 1 });

    expect(viewer.playerWrapper.rotation.y).toBeCloseTo(start + 0.5);
  });

  it("starts at three quarters so the cape shows from the side", async () => {
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    expect(Math.abs(lastViewer().playerWrapper.rotation.y)).toBeGreaterThan(0.3);
  });

  it("wears the cape, and swaps it without rebuilding the viewer", async () => {
    const { rerender } = render(
      <SkinViewer texture={TEXTURE} slim={false} cape={CAPE} label="Skin" />,
    );
    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();
    expect(viewer.options.cape).toBe(CAPE);

    rerender(<SkinViewer texture={TEXTURE} slim={false} cape={OTHER_CAPE} label="Skin" />);
    const image = loadImage(OTHER_CAPE);
    await waitFor(() => expect(viewer.loadCape).toHaveBeenLastCalledWith(image));
    rerender(<SkinViewer texture={TEXTURE} slim={false} cape={null} label="Skin" />);
    await waitFor(() => expect(viewer.loadCape).toHaveBeenLastCalledWith(null));
    expect(fakes.viewers).toHaveLength(1);
  });

  it("never puts a cape on a viewer that is gone, or an older cape over a newer one", async () => {
    const { rerender, unmount } = render(
      <SkinViewer texture={TEXTURE} slim={false} cape={CAPE} label="Skin" />,
    );
    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();

    rerender(<SkinViewer texture={TEXTURE} slim={false} cape={OTHER_CAPE} label="Skin" />);
    rerender(<SkinViewer texture={TEXTURE} slim={false} cape={CAPE} label="Skin" />);
    const newer = loadImage(CAPE);
    loadImage(OTHER_CAPE);
    await waitFor(() => expect(viewer.loadCape).toHaveBeenCalledOnce());
    expect(viewer.loadCape).toHaveBeenCalledWith(newer);

    rerender(<SkinViewer texture={TEXTURE} slim={false} cape={OTHER_CAPE} label="Skin" />);
    unmount();
    loadImage(OTHER_CAPE);
    await Promise.resolve();
    expect(viewer.loadCape).toHaveBeenCalledOnce();
  });

  it("takes the cape off when its image does not load", async () => {
    const { rerender } = render(
      <SkinViewer texture={TEXTURE} slim={false} cape={CAPE} label="Skin" />,
    );
    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();

    rerender(<SkinViewer texture={TEXTURE} slim={false} cape={OTHER_CAPE} label="Skin" />);
    const image = fakes.images.findLast((candidate) => candidate.src === OTHER_CAPE);
    image?.onerror?.();
    await waitFor(() => expect(viewer.loadCape).toHaveBeenLastCalledWith(null));
    expect(viewer.loadCape).toHaveBeenCalledOnce();
  });

  it("keeps the key light on the camera, so the side facing the player stays lit", async () => {
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    expect(lastViewer().camera.add).toHaveBeenCalledWith(expect.any(fakes.FakeLight));
  });

  it("uses the slim model when the skin is slim", async () => {
    render(<SkinViewer texture={TEXTURE} slim label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    expect(lastViewer().options.model).toBe("slim");
  });

  it("breathes when motion is allowed, but never turns by itself", async () => {
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();
    expect(viewer.animation).toBeInstanceOf(fakes.FakeFunctionAnimation);
    expect(viewer.autoRotate).toBe(false);
  });

  it("stands still, arms down and cape out, under reduced motion", async () => {
    mockReducedMotion(true);
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();
    expect(viewer.animation).toBeNull();
    expect(viewer.autoRotate).toBe(false);
    expect(Math.abs(viewer.playerObject.skin.leftArm.rotation.z)).toBeLessThan(0.2);
    expect(viewer.playerObject.cape.rotation.x).toBeGreaterThan(0);
  });

  it("turns with the buttons, a step each way", async () => {
    mockReducedMotion(true);
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);
    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();
    const start = viewer.playerWrapper.rotation.y;

    fireEvent.click(screen.getByRole("button", { name: es.accounts.turnRight }));
    const right = viewer.playerWrapper.rotation.y;
    expect(right).not.toBeCloseTo(start);
    fireEvent.click(screen.getByRole("button", { name: es.accounts.turnLeft }));
    expect(viewer.playerWrapper.rotation.y).toBeCloseTo(start);
  });

  it("eases a button turn over the next frames when motion is allowed", async () => {
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);
    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();
    const start = viewer.playerWrapper.rotation.y;

    fireEvent.click(screen.getByRole("button", { name: es.accounts.turnRight }));
    const animation = viewer.animation as InstanceType<typeof fakes.FakeFunctionAnimation>;
    animation.fn(viewer.playerObject, 0, 0.016);
    const partway = viewer.playerWrapper.rotation.y;
    expect(partway).not.toBeCloseTo(start);
    for (let frame = 0; frame < 120; frame += 1) animation.fn(viewer.playerObject, 0, 0.016);
    const settled = viewer.playerWrapper.rotation.y;
    expect(Math.abs(settled - start)).toBeGreaterThan(Math.abs(partway - start));
  });

  it("disposes the viewer and releases its WebGL context on unmount", async () => {
    const { unmount } = render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);
    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();

    unmount();

    expect(viewer.dispose).toHaveBeenCalledTimes(1);
    expect(viewer.renderer.forceContextLoss).toHaveBeenCalledTimes(1);
    expect(viewer.canvas.isConnected).toBe(false);
  });

  it("replaces the viewer when the texture changes", async () => {
    const { rerender } = render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);
    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const first = lastViewer();

    rerender(<SkinViewer texture={OTHER_TEXTURE} slim={false} label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(2));
    expect(first.dispose).toHaveBeenCalledTimes(1);
    expect(lastViewer().options.skin).toBe(OTHER_TEXTURE);
  });

  it("creates no viewer for a null texture but keeps the frame", async () => {
    render(<SkinViewer texture={null} slim={false} label="No skin" />);

    expect(screen.getByRole("img", { name: "No skin" })).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(fakes.viewers).toHaveLength(0);
    expect(screen.getByRole("img").querySelector("canvas")).toBeNull();
  });

  it("never creates a viewer when unmounted before the library loads", async () => {
    const { unmount } = render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);
    unmount();

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(fakes.viewers).toHaveLength(0);
  });

  it("releases the viewer when its setup throws", async () => {
    fakes.control.failSetup = true;
    try {
      const before = fakes.viewers.length;
      const { container } = render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);
      await waitFor(() => expect(fakes.viewers.length).toBe(before + 1));
      const viewer = fakes.viewers[before];
      if (!viewer) throw new Error("no viewer");
      await waitFor(() => expect(viewer.dispose).toHaveBeenCalledOnce());
      expect(viewer.renderer.forceContextLoss).toHaveBeenCalledOnce();
      expect(container.querySelector("canvas")).toBeNull();
    } finally {
      fakes.control.failSetup = false;
    }
  });
});
