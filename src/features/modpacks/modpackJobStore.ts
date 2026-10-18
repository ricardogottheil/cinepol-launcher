import { create } from "zustand";
import {
  type ClientErrorCode,
  importMrpack,
  installModpack,
  type ModpackInstallDto,
  type ProgressEvent,
  type ProgressStageDto,
  type ProgressUnitDto,
  type RunningOperation,
  toIpcError,
} from "../../ipc/client";

/** The one modpack install the launcher runs at a time. */
export type ModpackJob =
  | { kind: "idle" }
  | {
      kind: "installing";
      /** The pack being installed, or `null` for a local file. */
      title: string | null;
      stage: ProgressStageDto | null;
      done: number;
      total: number | null;
      unit: ProgressUnitDto;
      cancel: () => Promise<boolean>;
    };

interface ModpackJobState {
  job: ModpackJob;
  error: ClientErrorCode | null;
  /** The last instance a pack was installed into. */
  installed: ModpackInstallDto | null;
  installVersion: (project: string, version: string, title: string) => Promise<void>;
  importFile: () => Promise<void>;
}

/** Client-only state of modpack installs, kept across page changes. */
export const useModpackJobStore = create<ModpackJobState>((set) => {
  const run = async (
    title: string | null,
    start: (
      onProgress: (event: ProgressEvent) => void,
    ) => RunningOperation<ModpackInstallDto | null>,
  ) => {
    const operation = start((event) =>
      set({
        job: {
          kind: "installing",
          title,
          stage: event.stage,
          done: event.done,
          total: event.total,
          unit: event.unit,
          cancel: operation.cancel,
        },
      }),
    );
    set({
      job: {
        kind: "installing",
        title,
        stage: null,
        done: 0,
        total: null,
        unit: "steps",
        cancel: operation.cancel,
      },
      error: null,
      installed: null,
    });
    try {
      const installed = await operation.done;
      set({ job: { kind: "idle" }, installed });
    } catch (error) {
      set({ job: { kind: "idle" }, error: toIpcError(error).code });
    }
  };

  return {
    job: { kind: "idle" },
    error: null,
    installed: null,
    installVersion: (project, version, title) =>
      run(title, (onProgress) => installModpack(project, version, onProgress)),
    importFile: () => run(null, (onProgress) => importMrpack(onProgress)),
  };
});
