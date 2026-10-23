import { create } from "zustand";
import {
  type ClientErrorCode,
  type InstanceDto,
  importCurseForgeZip,
  importMrpack,
  importPackwiz,
  installModpack,
  installPackwiz,
  type ManualFileDto,
  type ModpackCatalog,
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

/** The last pack installed, and how many of its files wait for a manual download. */
export interface InstalledPack {
  instance: InstanceDto;
  pending: number;
}

interface ModpackJobState {
  job: ModpackJob;
  error: ClientErrorCode | null;
  installed: InstalledPack | null;
  installVersion: (
    source: ModpackCatalog,
    project: string,
    version: string,
    title: string,
  ) => Promise<void>;
  importFile: () => Promise<void>;
  /** Asks for a CurseForge pack zip and installs it. */
  importCurseForgeFile: () => Promise<void>;
  /** Installs the packwiz pack at an https `pack.toml` URL. */
  installPackwizUrl: (url: string) => Promise<void>;
  /** Asks for a local `pack.toml` and installs it. */
  importPackwizFile: () => Promise<void>;
}

/** What every install command resolves with (the packwiz ones add `pending`). */
interface InstallResult {
  instance: InstanceDto;
  pending?: ManualFileDto[];
}

/** Client-only state of modpack installs, kept across page changes. */
export const useModpackJobStore = create<ModpackJobState>((set) => {
  const run = async (
    title: string | null,
    start: (onProgress: (event: ProgressEvent) => void) => RunningOperation<InstallResult | null>,
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
      const result = await operation.done;
      set({
        job: { kind: "idle" },
        installed: result
          ? { instance: result.instance, pending: result.pending?.length ?? 0 }
          : null,
      });
    } catch (error) {
      set({ job: { kind: "idle" }, error: toIpcError(error).code });
    }
  };

  return {
    job: { kind: "idle" },
    error: null,
    installed: null,
    installVersion: (source, project, version, title) =>
      run(title, (onProgress) => installModpack(source, project, version, onProgress)),
    importFile: () => run(null, (onProgress) => importMrpack(onProgress)),
    importCurseForgeFile: () => run(null, (onProgress) => importCurseForgeZip(onProgress)),
    installPackwizUrl: (url) => run(url, (onProgress) => installPackwiz(url, onProgress)),
    importPackwizFile: () => run(null, (onProgress) => importPackwiz(onProgress)),
  };
});
