import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { Toaster } from "./Toaster";
import { MAX_TOASTS, TOAST_MS, toast, useToastStore } from "./toastStore";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Toaster", () => {
  it("announces a toast and lets it go after a while", () => {
    render(<Toaster />);
    act(() => {
      toast("errors.cancelled");
    });
    expect(screen.getByRole("status")).toHaveTextContent(esCommon.errors.cancelled);

    act(() => vi.advanceTimersByTime(TOAST_MS));
    expect(screen.queryByText(esCommon.errors.cancelled)).not.toBeInTheDocument();
  });

  it("closes a toast on demand", () => {
    render(<Toaster />);
    act(() => {
      toast("instances.updated", { tone: "success" });
    });
    fireEvent.click(screen.getByRole("button", { name: esCommon.toast.dismiss }));
    expect(screen.queryByText(esCommon.instances.updated)).not.toBeInTheDocument();
  });

  it("stays while the pointer rests on it", () => {
    render(<Toaster />);
    act(() => {
      toast("instances.updated", { tone: "success" });
    });
    fireEvent.mouseEnter(screen.getByText(esCommon.instances.updated));
    act(() => vi.advanceTimersByTime(TOAST_MS * 2));
    expect(screen.getByText(esCommon.instances.updated)).toBeInTheDocument();

    fireEvent.mouseLeave(screen.getByText(esCommon.instances.updated));
    act(() => vi.advanceTimersByTime(TOAST_MS));
    expect(screen.queryByText(esCommon.instances.updated)).not.toBeInTheDocument();
  });

  it("fills its message from the values it came with", () => {
    render(<Toaster />);
    act(() => {
      toast("instances.exported", { tone: "success", values: { path: "/tmp/a.mrpack" } });
    });
    expect(
      screen.getByText(esCommon.instances.exported.replace("{{path}}", "/tmp/a.mrpack")),
    ).toBeInTheDocument();
  });

  it("keeps only the newest toasts", () => {
    for (let index = 0; index < MAX_TOASTS + 2; index++) toast("errors.cancelled");
    expect(useToastStore.getState().toasts).toHaveLength(MAX_TOASTS);
  });
});
