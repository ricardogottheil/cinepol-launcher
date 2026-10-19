import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  IpcError,
  type LaunchOutcomeDto,
  type LogLineDto,
  type ProgressEvent,
} from "../../ipc/client";
import { CINEPOL_SESSION, useActivityStore } from "./activityStore";

interface PlayHandlers {
  onProgress: (event: ProgressEvent) => void;
  onLog: (line: LogLineDto) => void;
}

const client = vi.hoisted(() => ({ playOnCinepol: vi.fn() }));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const session = () => useActivityStore.getState().byInstance[CINEPOL_SESSION];

beforeEach(() => {
  useActivityStore.setState({ byInstance: {} });
});

describe("playCinepol", () => {
  it("goes from installing to running to idle with the exit recorded", async () => {
    const done = deferred<LaunchOutcomeDto>();
    let handlers: PlayHandlers | undefined;
    client.playOnCinepol.mockImplementation((options: PlayHandlers) => {
      handlers = options;
      return { operationId: "op", done: done.promise, cancel: vi.fn() };
    });

    const result = useActivityStore.getState().playCinepol();
    expect(session()?.activity.kind).toBe("installing");

    handlers?.onProgress({ stage: "libraries", done: 3, total: 10, unit: "bytes" });
    expect(session()?.activity).toMatchObject({ kind: "installing", stage: "libraries", done: 3 });

    handlers?.onProgress({ stage: "launching", done: 0, total: 1, unit: "steps" });
    handlers?.onLog({
      stream: "stdout",
      line: "Setting user: polispol1",
      level: null,
      thread: null,
      logger: null,
    });
    expect(session()?.activity.kind).toBe("running");
    expect(session()?.logs).toHaveLength(1);

    const outcome: LaunchOutcomeDto = {
      instanceId: "cinepol",
      exitCode: 0,
      killed: false,
      playedSecs: 60,
    };
    done.resolve(outcome);
    await expect(result).resolves.toEqual(outcome);
    expect(session()).toMatchObject({ activity: { kind: "idle" }, lastExit: outcome, error: null });
  });

  it("keeps the error code when the session fails", async () => {
    client.playOnCinepol.mockReturnValue({
      operationId: "op",
      done: Promise.reject(new IpcError("network", "offline")),
      cancel: vi.fn(),
    });
    await expect(useActivityStore.getState().playCinepol()).resolves.toBeNull();
    expect(session()).toMatchObject({ activity: { kind: "idle" }, error: "network" });
  });
});
