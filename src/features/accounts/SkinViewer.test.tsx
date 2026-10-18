import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SkinViewer } from "./SkinViewer";

interface ViewerOptions {
  skin?: string;
  model?: string;
  enableControls?: boolean;
  fov?: number;
}

const fakes = vi.hoisted(() => {
  const viewers: FakeViewer[] = [];

  class FakeFunctionAnimation {
    constructor(readonly fn: unknown) {}
  }

  class FakeViewer {
    readonly options: ViewerOptions;
    readonly canvas = document.createElement("canvas");
    readonly scene = { add: vi.fn() };
    readonly globalLight = { intensity: 3 };
    readonly cameraLight = { intensity: 0.6 };
    readonly playerObject = {
      position: { x: 0 },
      skin: { leftArm: { rotation: { z: 0 } } },
    };
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

  return { viewers, FakeViewer, FakeFunctionAnimation, FakeLight };
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

function lastViewer() {
  const viewer = fakes.viewers.at(-1);
  if (!viewer) throw new Error("no viewer was created");
  return viewer;
}

beforeEach(() => {
  fakes.viewers.length = 0;
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

  it("creates the viewer with the texture and the default model, without controls", async () => {
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    expect(lastViewer().options).toMatchObject({
      skin: TEXTURE,
      model: "default",
      enableControls: false,
      fov: 28,
    });
  });

  it("uses the slim model when the skin is slim", async () => {
    render(<SkinViewer texture={TEXTURE} slim label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    expect(lastViewer().options.model).toBe("slim");
  });

  it("waves when motion is allowed", async () => {
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    expect(lastViewer().animation).toBeInstanceOf(fakes.FakeFunctionAnimation);
  });

  it("holds a static raised-arm pose under reduced motion", async () => {
    mockReducedMotion(true);
    render(<SkinViewer texture={TEXTURE} slim={false} label="Skin" />);

    await waitFor(() => expect(fakes.viewers).toHaveLength(1));
    const viewer = lastViewer();
    expect(viewer.animation).toBeNull();
    expect(viewer.playerObject.skin.leftArm.rotation.z).toBeGreaterThan(Math.PI / 2);
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
});
