import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError, type ProgressDemoOptions } from "../../ipc/client";
import { ProgressDemo } from "./ProgressDemo";

const runProgressDemo = vi.hoisted(() => vi.fn());

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  runProgressDemo,
}));

const labels = esCommon.designKit.ipc;

function deferred() {
  let resolve: () => void = () => {};
  let reject: (error: unknown) => void = () => {};
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("ProgressDemo", () => {
  it("fills the meter from channel progress and reports completion", async () => {
    const done = deferred();
    let options: ProgressDemoOptions | undefined;
    runProgressDemo.mockImplementation((received: ProgressDemoOptions) => {
      options = received;
      return { operationId: "op", done: done.promise, cancel: vi.fn() };
    });

    render(<ProgressDemo />);
    fireEvent.click(screen.getByRole("button", { name: labels.start }));
    expect(screen.getByRole("status")).toHaveTextContent(labels.state.running);

    act(() => options?.onProgress({ done: 5, total: 20 }));
    expect(screen.getByRole("progressbar", { name: labels.progressLabel })).toHaveAttribute(
      "aria-valuenow",
      "5",
    );

    await act(async () => done.resolve());
    expect(screen.getByRole("status")).toHaveTextContent(labels.state.done);
  });

  it("cancels the running operation and shows the cancelled state", async () => {
    const done = deferred();
    const cancel = vi.fn(() => {
      done.reject(new IpcError("cancelled", "the operation was cancelled"));
      return Promise.resolve(true);
    });
    runProgressDemo.mockReturnValue({ operationId: "op", done: done.promise, cancel });

    render(<ProgressDemo />);
    fireEvent.click(screen.getByRole("button", { name: labels.start }));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: labels.cancel }));
    });

    expect(cancel).toHaveBeenCalledOnce();
    expect(screen.getByRole("status")).toHaveTextContent(labels.state.cancelled);
  });
});
