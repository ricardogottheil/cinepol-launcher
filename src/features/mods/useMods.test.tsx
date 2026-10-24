import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { IpcError, type ProgressEvent } from "../../ipc/client";
import { createTestQueryClient } from "../../test/render";
import {
  useInstanceMods,
  useModInstallPreview,
  useRemoveMod,
  useSetModEnabled,
  useWriteMod,
} from "./useMods";

const client = vi.hoisted(() => ({
  installMod: vi.fn(),
  updateMod: vi.fn(),
  listInstanceMods: vi.fn(),
  checkModUpdates: vi.fn(),
  previewModInstall: vi.fn(),
  removeMod: vi.fn(),
  setModEnabled: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const iris = { project: "iris", title: "Iris", versionNumber: "1.8.8", filename: "iris.jar" };
const progress: ProgressEvent = { stage: "mods", done: 1, total: 2, unit: "steps" };

function setup<T>(hook: () => T) {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return renderHook(hook, { wrapper });
}

/** An operation that reports `progress` and stays running until `finish` is called. */
function pendingOperation() {
  let finish: (value: unknown) => void = () => {};
  const done = new Promise((resolve) => {
    finish = resolve;
  });
  const cancel = vi.fn(async () => true);
  return {
    start: (_id: string, _project: string, onProgress: (event: ProgressEvent) => void) => {
      onProgress(progress);
      return { operationId: "op", done, cancel };
    },
    finish,
    cancel,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  client.listInstanceMods.mockResolvedValue([]);
});

describe("mod hooks", () => {
  it("installs a mod with its progress, can cancel it, and rereads the installed list", async () => {
    const operation = pendingOperation();
    client.installMod.mockImplementation(operation.start);
    const { result } = setup(() => ({
      mods: useInstanceMods("survival"),
      write: useWriteMod("survival"),
    }));
    await waitFor(() => expect(result.current.mods.isSuccess).toBe(true));
    expect(client.listInstanceMods).toHaveBeenCalledTimes(1);

    act(() => result.current.write.mutate({ project: "iris" }));
    await waitFor(() => expect(result.current.write.progress).toEqual(progress));
    expect(client.installMod).toHaveBeenCalledWith("survival", "iris", expect.any(Function));
    expect(client.updateMod).not.toHaveBeenCalled();
    await expect(result.current.write.cancel()).resolves.toBe(true);
    expect(operation.cancel).toHaveBeenCalledTimes(1);

    await act(async () => operation.finish([iris]));
    await waitFor(() => expect(result.current.write.data).toEqual([iris]));
    expect(result.current.write.progress).toBeNull();
    await waitFor(() => expect(client.listInstanceMods).toHaveBeenCalledTimes(2));
    await expect(result.current.write.cancel()).resolves.toBe(false);
  });

  it("updates through update_mod when asked to", async () => {
    const operation = pendingOperation();
    client.updateMod.mockImplementation(operation.start);
    const { result } = setup(() => useWriteMod("survival"));
    act(() => result.current.mutate({ project: "iris", update: true }));
    await act(async () => operation.finish([iris]));
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(client.updateMod).toHaveBeenCalledWith("survival", "iris", expect.any(Function));
    expect(client.installMod).not.toHaveBeenCalled();
  });

  it("rereads the installed list after a refused removal or toggle too", async () => {
    client.removeMod.mockRejectedValue(new IpcError("already_running", "busy"));
    client.setModEnabled.mockRejectedValue(new IpcError("already_running", "busy"));
    const { result } = setup(() => ({
      mods: useInstanceMods("survival"),
      remove: useRemoveMod("survival"),
      toggle: useSetModEnabled("survival"),
    }));
    await waitFor(() => expect(result.current.mods.isSuccess).toBe(true));

    act(() => result.current.remove.mutate({ filename: "iris.jar", removeOrphans: true }));
    await waitFor(() => expect(result.current.remove.isError).toBe(true));
    expect(client.removeMod).toHaveBeenCalledWith("survival", "iris.jar", true);
    await waitFor(() => expect(client.listInstanceMods).toHaveBeenCalledTimes(2));

    act(() => result.current.toggle.mutate({ filename: "iris.jar", enabled: false }));
    await waitFor(() => expect(result.current.toggle.isError).toBe(true));
    expect(client.setModEnabled).toHaveBeenCalledWith("survival", "iris.jar", false);
    await waitFor(() => expect(client.listInstanceMods).toHaveBeenCalledTimes(3));
  });

  it("previews an install only once a mod is picked", async () => {
    client.previewModInstall.mockResolvedValue({ mods: [] });
    const { result } = setup(() => useModInstallPreview("survival", null));
    expect(result.current.fetchStatus).toBe("idle");
    expect(client.previewModInstall).not.toHaveBeenCalled();

    const picked = setup(() => useModInstallPreview("survival", "iris"));
    await waitFor(() => expect(picked.result.current.isSuccess).toBe(true));
    expect(client.previewModInstall).toHaveBeenCalledWith("survival", "iris");
  });
});
