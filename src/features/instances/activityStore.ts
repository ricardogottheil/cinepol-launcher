import { create } from "zustand";
import {
  type ClientErrorCode,
  type InstallSummaryDto,
  installInstance,
  killInstance,
  type LaunchOutcomeDto,
  type LogLineDto,
  launchInstance,
  type ProgressStageDto,
  toIpcError,
} from "../../ipc/client";

/** Lines kept per instance; older output is dropped. */
export const MAX_LOG_LINES = 2000;

export type Activity =
  | { kind: "idle" }
  | {
      kind: "installing";
      stage: ProgressStageDto | null;
      done: number;
      total: number | null;
      cancel: () => Promise<boolean>;
    }
  | { kind: "running" };

export interface InstanceActivity {
  activity: Activity;
  logs: LogLineDto[];
  error: ClientErrorCode | null;
  lastExit: LaunchOutcomeDto | null;
}

interface ActivityState {
  byInstance: Record<string, InstanceActivity>;
  install: (instanceId: string) => Promise<InstallSummaryDto | null>;
  play: (instanceId: string) => Promise<LaunchOutcomeDto | null>;
  stop: (instanceId: string) => Promise<void>;
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
          cancel: operation.cancel,
        },
      }),
    );
    update(instanceId, {
      activity: { kind: "installing", stage: null, done: 0, total: null, cancel: operation.cancel },
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

  const play = async (instanceId: string): Promise<LaunchOutcomeDto | null> => {
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
      if (code === "not_installed" && (await install(instanceId))) {
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

  return { byInstance: {}, install, play, stop };
});

export function useInstanceActivity(instanceId: string): InstanceActivity {
  return useActivityStore((state) => state.byInstance[instanceId] ?? IDLE);
}
