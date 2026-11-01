import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import type { InstanceDto } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { useWriteMod } from "../mods/useMods";
import { PlayControl } from "./InstanceActivity";
import { isCinepolInstance, useInstanceSession } from "./useInstanceSession";

const client = vi.hoisted(() => ({
  installMod: vi.fn(),
  listInstanceMods: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const fabric = {
  id: "survival",
  name: "Survival",
  gameVersion: "1.21.1",
  loader: "fabric",
  loaderVersion: "0.16.5",
  origin: "custom",
  modpackSource: null,
  updatePackOnLaunch: false,
  playtimeSecs: 0,
} as InstanceDto;

/** A mod writer on one screen and the instance's Play on another. */
function Screens({ instance = fabric }: { instance?: InstanceDto }) {
  const writer = useWriteMod("survival");
  const session = useInstanceSession(instance);
  return (
    <>
      <button type="button" onClick={() => writer.mutate({ project: "iris" })}>
        install
      </button>
      <PlayControl instance={instance} session={session} />
    </>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  client.listInstanceMods.mockResolvedValue([]);
});

describe("useInstanceSession", () => {
  it("keeps Play off while a mod is being installed into the instance", async () => {
    let finish: (value: unknown) => void = () => {};
    const done = new Promise((resolve) => {
      finish = resolve;
    });
    client.installMod.mockReturnValue({ operationId: "op", done, cancel: vi.fn() });
    renderWithQuery(<Screens />);
    const play = screen.getByRole("button", { name: esCommon.instances.play });
    expect(play).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "install" }));
    await waitFor(() => expect(play).toBeDisabled());

    await act(async () => finish([]));
    await waitFor(() => expect(play).toBeEnabled());
  });

  it("leaves Play alone while another instance gets a mod", async () => {
    client.installMod.mockReturnValue({
      operationId: "op",
      done: new Promise(() => {}),
      cancel: vi.fn(),
    });
    renderWithQuery(<Screens instance={{ ...fabric, id: "creative" }} />);
    fireEvent.click(screen.getByRole("button", { name: "install" }));

    await waitFor(() => expect(client.installMod).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: esCommon.instances.play })).toBeEnabled();
  });
});

const PACK = "https://pack.policarpiers.com/pack.toml";

const base: InstanceDto = {
  id: "a",
  name: "A",
  gameVersion: "26.2",
  loader: "vanilla",
  loaderVersion: null,
  origin: "custom",
  modpackSource: null,
  modpackProject: null,
  updatePackOnLaunch: false,
  playtimeSecs: 0,
};
const vanilla: InstanceDto = { ...base, origin: "cinepolServer" };
const pack: InstanceDto = {
  ...base,
  origin: "modpack",
  modpackSource: "packwiz",
  modpackProject: PACK,
};

describe("isCinepolInstance", () => {
  it("is the community pack's packwiz instance when the config names a pack", () => {
    expect(isCinepolInstance(pack, { pack: PACK })).toBe(true);
    expect(isCinepolInstance(vanilla, { pack: PACK })).toBe(false);
    expect(
      isCinepolInstance(
        { ...pack, modpackProject: "https://example.org/pack.toml" },
        { pack: PACK },
      ),
    ).toBe(false);
    expect(isCinepolInstance({ ...pack, modpackSource: "modrinth" }, { pack: PACK })).toBe(false);
  });

  it("is the managed Vanilla instance without a pack", () => {
    expect(isCinepolInstance(vanilla, { pack: null })).toBe(true);
    expect(isCinepolInstance(pack, { pack: null })).toBe(false);
    expect(isCinepolInstance(vanilla, undefined)).toBe(true);
  });
});
