import { create } from "zustand";
import {
  type ClientErrorCode,
  type DiagnosticDto,
  diagnoseServer,
  type SharedReportDto,
  shareDiagnosticReport,
  toIpcError,
} from "../../ipc/client";

/** The offered durations in seconds; 5 minutes catches intermittent loss. */
export const DURATIONS = [60, 300, 600] as const;
export const DEFAULT_DURATION = 300;

interface DiagnosticsState {
  /** The server picked for the next (or current) test. */
  address: string;
  durationSecs: number;
  diagnostic: DiagnosticDto | null;
  startedAt: Date | null;
  running: boolean;
  error: ClientErrorCode | null;
  /** Show the player's router and provider in the table and the report. */
  showPrivate: boolean;
  shared: SharedReportDto | null;
  sharing: boolean;
  shareError: ClientErrorCode | null;
  cancel: (() => Promise<boolean>) | null;
  select: (address: string) => void;
  setDuration: (durationSecs: number) => void;
  setShowPrivate: (show: boolean) => void;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  share: (report: string) => Promise<void>;
}

/**
 * The network test, kept outside the page so leaving it never stops the
 * test nor loses its result.
 */
export const useDiagnostics = create<DiagnosticsState>((set, get) => ({
  address: "",
  durationSecs: DEFAULT_DURATION,
  diagnostic: null,
  startedAt: null,
  running: false,
  error: null,
  showPrivate: false,
  shared: null,
  sharing: false,
  shareError: null,
  cancel: null,
  select: (address) => {
    if (get().running) return;
    set({ address });
  },
  setDuration: (durationSecs) => {
    if (get().running) return;
    set({ durationSecs });
  },
  // A link made with the other setting would not match what is shown.
  setShowPrivate: (showPrivate) => set({ showPrivate, shared: null, shareError: null }),
  start: async () => {
    const { address, durationSecs, running } = get();
    if (running || address.trim() === "") return;
    const operation = diagnoseServer(address.trim(), durationSecs, (diagnostic) =>
      set({ diagnostic }),
    );
    set({
      running: true,
      diagnostic: null,
      startedAt: new Date(),
      error: null,
      shared: null,
      shareError: null,
      cancel: operation.cancel,
    });
    try {
      const diagnostic = await operation.done;
      set({ diagnostic });
    } catch (failure) {
      set({ error: toIpcError(failure).code });
    } finally {
      set({ running: false, cancel: null });
    }
  },
  stop: async () => {
    await get().cancel?.();
  },
  share: async (report) => {
    if (get().sharing) return;
    set({ sharing: true, shareError: null });
    try {
      set({ shared: await shareDiagnosticReport(report) });
    } catch (failure) {
      set({ shareError: toIpcError(failure).code });
    } finally {
      set({ sharing: false });
    }
  },
}));
