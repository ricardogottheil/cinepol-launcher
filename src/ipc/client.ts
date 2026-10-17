import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type { AppError, AppInfo, Commands, ErrorCode, ProgressEvent } from "./bindings";

/**
 * The only module that talks to the backend. Components and hooks call these
 * typed functions; they never import `bindings.ts` or `@tauri-apps/api/core`.
 */

export type { AppInfo, ErrorCode, ProgressEvent };

/** Backend error codes plus the one the client raises on its own. */
export type ClientErrorCode = ErrorCode | "ipc_unavailable";

const BACKEND_CODES = {
  cancelled: true,
  invalid_input: true,
  internal: true,
} as const satisfies Record<ErrorCode, true>;

/** A failed command. Switch on `code`; `message` is for logs only. */
export class IpcError extends Error {
  readonly code: ClientErrorCode;
  readonly details: string | undefined;

  constructor(code: ClientErrorCode, message: string, details?: string) {
    super(message);
    this.name = "IpcError";
    this.code = code;
    this.details = details;
  }
}

function isAppError(value: unknown): value is AppError {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    Object.hasOwn(BACKEND_CODES, candidate.code) &&
    typeof candidate.message === "string"
  );
}

/** Normalizes anything a command rejects with into an `IpcError`. */
export function toIpcError(error: unknown): IpcError {
  if (error instanceof IpcError) return error;
  if (isAppError(error)) return new IpcError(error.code, error.message, error.details);
  return new IpcError("internal", error instanceof Error ? error.message : String(error));
}

type ArgsOf<K extends keyof Commands> =
  Commands[K]["args"] extends Record<string, never> ? [] : [Commands[K]["args"]];

async function call<K extends keyof Commands>(
  command: K,
  ...args: ArgsOf<K>
): Promise<Commands[K]["result"]> {
  if (!isTauri()) {
    throw new IpcError("ipc_unavailable", `The desktop backend is not available (${command})`);
  }
  try {
    return await invoke<Commands[K]["result"]>(command, args[0]);
  } catch (error) {
    throw toIpcError(error);
  }
}

export function getAppInfo(): Promise<AppInfo> {
  return call("get_app_info");
}

/** A long operation started from the UI. */
export interface RunningOperation {
  operationId: string;
  /** Resolves on success; rejects with an `IpcError` (code `cancelled` after `cancel`). */
  done: Promise<void>;
  /** Requests cancellation. Resolves `false` when the operation already finished. */
  cancel: () => Promise<boolean>;
}

export interface ProgressDemoOptions {
  steps: number;
  intervalMs: number;
  onProgress: (event: ProgressEvent) => void;
}

export function runProgressDemo({
  steps,
  intervalMs,
  onProgress,
}: ProgressDemoOptions): RunningOperation {
  const operationId = crypto.randomUUID();
  const cancel = () => call("cancel_operation", { operationId });

  if (!isTauri()) {
    const unavailable = new IpcError(
      "ipc_unavailable",
      "The desktop backend is not available (run_progress_demo)",
    );
    return { operationId, done: Promise.reject(unavailable), cancel };
  }

  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  const done = call("run_progress_demo", {
    operationId,
    steps,
    intervalMs,
    onProgress: channel,
  }).then(() => undefined);
  return { operationId, done, cancel };
}
