import { create } from "zustand";
import {
  type ClientErrorCode,
  exportMrpack,
  type InstallSummaryDto,
  installInstance,
  killInstance,
  type LaunchOutcomeDto,
  type LogLineDto,
  launchInstance,
  type ProgressEvent,
  type ProgressStageDto,
  type ProgressUnitDto,
  playOnCinepol,
  toIpcError,
  updateModpack,
  updatePackwiz,
} from "../../ipc/client";

/** Store key of the "Play on CINEPOL" session (instance ids never contain a colon). */
export const CINEPOL_SESSION = "cinepol:session";

/** Lines kept per instance; older output is dropped. */
export const MAX_LOG_LINES = 2000;

export type Activity =
  | { kind: "idle" }
  | {
      kind: "installing";
      stage: ProgressStageDto | null;
      done: number;
      total: number | null;
      /** What `done` and `total` count. */
      unit: ProgressUnitDto;
      cancel: () => Promise<boolean>;
    }
  | { kind: "exporting" }
  | {
      kind: "running";
      /** Stops a game started by an operation (Play on CINEPOL); per-instance games use `stop`. */
      cancel?: () => Promise<boolean>;
    };

/** The catalogs an instance can be updated from. */
/** Packwiz syncs with its pack; Modrinth and CurseForge update to their newest version. */
export type PackSource = "modrinth" | "curseforge" | "packwiz";

/** The outcome of the last modpack update or export, shown under the row. */
export type ActivityNotice =
  | { kind: "updated"; kept: number; pending: number }
  | { kind: "upToDate" }
  | { kind: "exported"; path: string };

export interface InstanceActivity {
  activity: Activity;
  logs: LogLineDto[];
  error: ClientErrorCode | null;
  lastExit: LaunchOutcomeDto | null;
  notice?: ActivityNotice | null;
}

interface ActivityState {
  byInstance: Record<string, InstanceActivity>;
  install: (instanceId: string) => Promise<InstallSummaryDto | null>;
  /** `syncPack` updates a packwiz instance first, and plays only if that worked. */
  play: (instanceId: string, options?: { syncPack?: boolean }) => Promise<LaunchOutcomeDto | null>;
  stop: (instanceId: string) => Promise<void>;
  /** Updates a modpack instance in place, keeping the player's changes. */
  /** Resolves with what the update left to download by hand, or `null` when it failed. */
  updatePack: (instanceId: string, source?: PackSource) => Promise<{ pending: number } | null>;
  /** Asks where to save and exports the instance as a `.mrpack`. */
  exportPack: (instanceId: string) => Promise<void>;
  playCinepol: () => Promise<LaunchOutcomeDto | null>;
}

const IDLE: InstanceActivity = {
  activity: { kind: "idle" },
  logs: [],
  error: null,
  lastExit: null,
};

/** Client-only state of installs and running games, kept across page changes. */
export const useActivityStore = create<ActivityState>((set) => {
  const update = (instanceId: string, patch: Partial<InstanceActivity>) =>
    set((state) => ({
      byInstance: {
        ...state.byInstance,
        [instanceId]: { ...(state.byInstance[instanceId] ?? IDLE), ...patch },
      },
    }));

  const install = async (instanceId: string): Promise<InstallSummaryDto | null> => {
    const operation = installInstance(instanceId, (event) =>
      update(instanceId, {
        activity: {
          kind: "installing",
          stage: event.stage,
          done: event.done,
          total: event.total,
          unit: event.unit,
          cancel: operation.cancel,
        },
      }),
    );
    update(instanceId, {
      activity: {
        kind: "installing",
        stage: null,
        done: 0,
        total: null,
        unit: "steps",
        cancel: operation.cancel,
      },
      error: null,
    });
    try {
      const summary = await operation.done;
      update(instanceId, { activity: { kind: "idle" } });
      return summary;
    } catch (error) {
      update(instanceId, { activity: { kind: "idle" }, error: toIpcError(error).code });
      return null;
    }
  };

  const updatePack = async (instanceId: string, source: PackSource = "modrinth") => {
    const onProgress = (event: ProgressEvent) =>
      update(instanceId, {
        activity: {
          kind: "installing",
          stage: event.stage,
          done: event.done,
          total: event.total,
          unit: event.unit,
          cancel: operation.cancel,
        },
      });
    const operation =
      source === "packwiz"
        ? updatePackwiz(instanceId, onProgress)
        : updateModpack(instanceId, onProgress);
    update(instanceId, {
      activity: {
        kind: "installing",
        stage: null,
        done: 0,
        total: null,
        unit: "steps",
        cancel: operation.cancel,
      },
      error: null,
      notice: null,
    });
    try {
      const outcome = await operation.done;
      const pending = "pending" in outcome ? outcome.pending.length : 0;
      const notice: ActivityNotice =
        "upToDate" in outcome && outcome.upToDate
          ? { kind: "upToDate" }
          : { kind: "updated", kept: outcome.files?.kept ?? 0, pending };
      update(instanceId, { activity: { kind: "idle" }, notice });
      return { pending };
    } catch (error) {
      update(instanceId, { activity: { kind: "idle" }, error: toIpcError(error).code });
      return null;
    }
  };

  const exportPack = async (instanceId: string) => {
    update(instanceId, { activity: { kind: "exporting" }, error: null, notice: null });
    try {
      const exported = await exportMrpack(instanceId);
      update(instanceId, {
        activity: { kind: "idle" },
        notice: exported ? { kind: "exported", path: exported.path } : null,
      });
    } catch (error) {
      update(instanceId, { activity: { kind: "idle" }, error: toIpcError(error).code });
    }
  };

  const play = async (
    instanceId: string,
    options: { syncPack?: boolean } = {},
  ): Promise<LaunchOutcomeDto | null> => {
    if (options.syncPack) {
      const synced = await updatePack(instanceId, "packwiz");
      // A pack with files still to download by hand cannot start yet; the row
      // shows the list instead.
      if (!synced || synced.pending > 0) return null;
    }
    const appendLine = (line: LogLineDto) =>
      set((state) => {
        const current = state.byInstance[instanceId] ?? IDLE;
        return {
          byInstance: {
            ...state.byInstance,
            [instanceId]: { ...current, logs: [...current.logs, line].slice(-MAX_LOG_LINES) },
          },
        };
      });

    const launch = async () => {
      update(instanceId, { activity: { kind: "running" }, logs: [], error: null, lastExit: null });
      return launchInstance(instanceId, appendLine);
    };

    try {
      const outcome = await launch();
      update(instanceId, { activity: { kind: "idle" }, lastExit: outcome });
      return outcome;
    } catch (error) {
      const code = toIpcError(error).code;
      if (code === "not_installed") {
        // A failed install already recorded its own, more precise error.
        if (!(await install(instanceId))) return null;
        try {
          const outcome = await launch();
          update(instanceId, { activity: { kind: "idle" }, lastExit: outcome });
          return outcome;
        } catch (retryError) {
          update(instanceId, { activity: { kind: "idle" }, error: toIpcError(retryError).code });
          return null;
        }
      }
      update(instanceId, { activity: { kind: "idle" }, error: code });
      return null;
    }
  };

  const stop = async (instanceId: string) => {
    await killInstance(instanceId).catch(() => false);
  };

  const playCinepol = async (): Promise<LaunchOutcomeDto | null> => {
    const key = CINEPOL_SESSION;
    const operation = playOnCinepol({
      onProgress: (event) =>
        update(key, {
          activity:
            event.stage === "launching"
              ? { kind: "running", cancel: operation.cancel }
              : {
                  kind: "installing",
                  stage: event.stage,
                  done: event.done,
                  total: event.total,
                  unit: event.unit,
                  cancel: operation.cancel,
                },
        }),
      onLog: (line) =>
        set((state) => {
          const current = state.byInstance[key] ?? IDLE;
          return {
            byInstance: {
              ...state.byInstance,
              [key]: { ...current, logs: [...current.logs, line].slice(-MAX_LOG_LINES) },
            },
          };
        }),
    });
    update(key, {
      activity: {
        kind: "installing",
        stage: "metadata",
        done: 0,
        total: null,
        unit: "steps",
        cancel: operation.cancel,
      },
      logs: [],
      error: null,
      lastExit: null,
    });
    try {
      const outcome = await operation.done;
      update(key, { activity: { kind: "idle" }, lastExit: outcome });
      return outcome;
    } catch (error) {
      update(key, { activity: { kind: "idle" }, error: toIpcError(error).code });
      return null;
    }
  };

  return { byInstance: {}, install, play, stop, updatePack, exportPack, playCinepol };
});

export function useInstanceActivity(instanceId: string): InstanceActivity {
  return useActivityStore((state) => state.byInstance[instanceId] ?? IDLE);
}
