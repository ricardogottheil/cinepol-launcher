import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  invoke: vi.fn(),
  channels: [] as Array<{ onmessage: (message: unknown) => void }>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: tauri.isTauri,
  invoke: tauri.invoke,
  Channel: class {
    onmessage: (message: unknown) => void = () => {};
    constructor() {
      tauri.channels.push(this);
    }
  },
}));

import { getAppInfo, IpcError, runProgressDemo, toIpcError } from "./client";

beforeEach(() => {
  tauri.isTauri.mockReturnValue(true);
  tauri.invoke.mockReset();
  tauri.channels.length = 0;
});

describe("getAppInfo", () => {
  it("invokes get_app_info and returns its result", async () => {
    tauri.invoke.mockResolvedValue({ name: "Cinepol Launcher", version: "0.1.0" });
    await expect(getAppInfo()).resolves.toEqual({ name: "Cinepol Launcher", version: "0.1.0" });
    expect(tauri.invoke).toHaveBeenCalledWith("get_app_info", undefined);
  });

  it("rejects with ipc_unavailable outside the desktop app, without invoking", async () => {
    tauri.isTauri.mockReturnValue(false);
    await expect(getAppInfo()).rejects.toMatchObject({ code: "ipc_unavailable" });
    expect(tauri.invoke).not.toHaveBeenCalled();
  });
});

describe("error mapping", () => {
  it("keeps the code, message and details of a backend AppError", async () => {
    tauri.invoke.mockRejectedValue({
      code: "invalid_input",
      message: "bad steps",
      details: "steps=0",
    });
    const error = await getAppInfo().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(IpcError);
    expect(error).toMatchObject({
      code: "invalid_input",
      message: "bad steps",
      details: "steps=0",
    });
  });

  it("maps anything else to internal", () => {
    expect(toIpcError("boom")).toMatchObject({ code: "internal", message: "boom" });
    expect(toIpcError({ code: "not_a_code", message: "x" })).toMatchObject({ code: "internal" });
  });
});

describe("runProgressDemo", () => {
  it("passes camelCase arguments and forwards channel messages", async () => {
    tauri.invoke.mockResolvedValue(null);
    const onProgress = vi.fn();
    const operation = runProgressDemo({ steps: 3, intervalMs: 50, onProgress });

    expect(tauri.invoke).toHaveBeenCalledWith("run_progress_demo", {
      operationId: operation.operationId,
      steps: 3,
      intervalMs: 50,
      onProgress: tauri.channels[0],
    });
    tauri.channels[0]?.onmessage({ done: 1, total: 3 });
    expect(onProgress).toHaveBeenCalledWith({ done: 1, total: 3 });
    await expect(operation.done).resolves.toBeUndefined();
  });

  it("cancels its own operation id", async () => {
    tauri.invoke.mockResolvedValueOnce(null).mockResolvedValueOnce(true);
    const operation = runProgressDemo({ steps: 3, intervalMs: 50, onProgress: () => {} });
    await expect(operation.cancel()).resolves.toBe(true);
    expect(tauri.invoke).toHaveBeenLastCalledWith("cancel_operation", {
      operationId: operation.operationId,
    });
  });

  it("rejects done with ipc_unavailable outside the desktop app, without a channel", async () => {
    tauri.isTauri.mockReturnValue(false);
    const operation = runProgressDemo({ steps: 3, intervalMs: 50, onProgress: () => {} });
    await expect(operation.done).rejects.toMatchObject({ code: "ipc_unavailable" });
    expect(tauri.channels).toHaveLength(0);
    expect(tauri.invoke).not.toHaveBeenCalled();
  });

  it("rejects done with cancelled when the backend reports a cancellation", async () => {
    tauri.invoke.mockRejectedValue({ code: "cancelled", message: "the operation was cancelled" });
    const operation = runProgressDemo({ steps: 3, intervalMs: 50, onProgress: () => {} });
    await expect(operation.done).rejects.toMatchObject({ code: "cancelled" });
  });
});
