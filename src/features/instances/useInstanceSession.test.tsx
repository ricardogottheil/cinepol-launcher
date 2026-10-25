import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import type { InstanceDto } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { useWriteMod } from "../mods/useMods";
import { PlayControl } from "./InstanceActivity";
import { useInstanceSession } from "./useInstanceSession";

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
