import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  AccountDto,
  AccountPolicyDto,
  AccountsDto,
  AppError,
  AppInfo,
  Commands,
  DeviceCodeDto,
  ErrorCode,
  Events,
  GameVersionsDto,
  InstallSummaryDto,
  InstanceDto,
  LaunchOutcomeDto,
  LoaderReleaseDto,
  LogLineDto,
  ModpackDetailsDto,
  ModpackExportDto,
  ModpackInstallDto,
  ModpackPageDto,
  ModpackSummaryDto,
  ModpackUpdateDto,
  ModpackVersionDto,
  PackFilesDto,
  PremiumNameDto,
  ProgressEvent,
  ProgressStageDto,
  ProgressUnitDto,
  ServerConfigDto,
  ServerStatusDto,
  SettingsDto,
  SkinDto,
} from "./bindings";

/**
 * The only module that talks to the backend. Components and hooks call these
 * typed functions; they never import `bindings.ts` or `@tauri-apps/api/core`.
 */

export type {
  AccountDto,
  AccountPolicyDto,
  AccountsDto,
  AppInfo,
  DeviceCodeDto,
  ErrorCode,
  GameVersionsDto,
  InstallSummaryDto,
  InstanceDto,
  LaunchOutcomeDto,
  LoaderReleaseDto,
  LogLineDto,
  ModpackDetailsDto,
  ModpackExportDto,
  ModpackInstallDto,
  ModpackPageDto,
  ModpackSummaryDto,
  ModpackUpdateDto,
  ModpackVersionDto,
  PackFilesDto,
  PremiumNameDto,
  ProgressEvent,
  ProgressStageDto,
  ProgressUnitDto,
  ServerConfigDto,
  ServerStatusDto,
  SettingsDto,
  SkinDto,
};

/** Backend error codes plus the one the client raises on its own. */
export type ClientErrorCode = ErrorCode | "ipc_unavailable";

const BACKEND_CODES = {
  cancelled: true,
  invalid_input: true,
  not_found: true,
  already_exists: true,
  no_account: true,
  premium_required: true,
  not_installed: true,
  network: true,
  java_unavailable: true,
  already_running: true,
  server_unreachable: true,
  session_expired: true,
  sign_in_refused: true,
  not_entitled: true,
  app_not_approved: true,
  keychain_unavailable: true,
  microsoft_not_configured: true,
  loader_unavailable: true,
  unverified_file: true,
  rate_limited: true,
  invalid_modpack: true,
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

function unavailable(command: string): IpcError {
  return new IpcError("ipc_unavailable", `The desktop backend is not available (${command})`);
}

async function call<K extends keyof Commands>(
  command: K,
  ...args: ArgsOf<K>
): Promise<Commands[K]["result"]> {
  if (!isTauri()) throw unavailable(command);
  try {
    return await invoke<Commands[K]["result"]>(command, args[0]);
  } catch (error) {
    throw toIpcError(error);
  }
}

function channel<T>(onMessage: (message: T) => void): Channel<T> {
  const created = new Channel<T>();
  created.onmessage = onMessage;
  return created;
}

export function getAppInfo(): Promise<AppInfo> {
  return call("get_app_info");
}

/** A long operation started from the UI. */
export interface RunningOperation<T = void> {
  operationId: string;
  /** Resolves with the result; rejects with an `IpcError` (code `cancelled` after `cancel`). */
  done: Promise<T>;
  /** Requests cancellation. Resolves `false` when the operation already finished. */
  cancel: () => Promise<boolean>;
}

function startOperation<T>(
  command: keyof Commands,
  run: (operationId: string) => Promise<T>,
): RunningOperation<T> {
  const operationId = crypto.randomUUID();
  const cancel = () => call("cancel_operation", { operationId });
  if (!isTauri()) {
    return { operationId, done: Promise.reject(unavailable(command)), cancel };
  }
  return { operationId, done: run(operationId), cancel };
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
  return startOperation("run_progress_demo", (operationId) =>
    call("run_progress_demo", {
      operationId,
      steps,
      intervalMs,
      onProgress: channel(onProgress),
    }).then(() => undefined),
  );
}

export function listAccounts(): Promise<AccountsDto> {
  return call("list_accounts");
}

export function createOfflineAccount(username: string): Promise<AccountDto> {
  return call("create_offline_account", { username });
}

/** What the Accounts screen may offer in this build. */
export function getAccountPolicy(): Promise<AccountPolicyDto> {
  return call("get_account_policy");
}

/**
 * Signs in with Microsoft. `onCode` receives the code the player types at the
 * shown address; cancel the returned operation to stop waiting.
 */
export function loginMicrosoft(
  onCode: (code: DeviceCodeDto) => void,
): RunningOperation<AccountDto> {
  return startOperation("login_microsoft", (operationId) =>
    call("login_microsoft", { operationId, onCode: channel(onCode) }),
  );
}

/** Removes an account and its stored tokens. */
export function removeAccount(accountId: string): Promise<null> {
  return call("remove_account", { accountId });
}

/** The account's skin as a data URL, or `null` when it has none. */
export function getAccountSkin(accountId: string): Promise<SkinDto | null> {
  return call("get_account_skin", { accountId });
}

/** Whether a premium account owns the name (`unknown` when the lookup failed). */
export function checkPremiumName(username: string): Promise<PremiumNameDto> {
  return call("check_premium_name", { username });
}

export async function setActiveAccount(accountId: string): Promise<void> {
  await call("set_active_account", { accountId });
}

export function listGameVersions(includeSnapshots: boolean): Promise<GameVersionsDto> {
  return call("list_game_versions", { includeSnapshots });
}

export function listInstances(): Promise<InstanceDto[]> {
  return call("list_instances");
}

/** Loaders an instance can be created with in this version of the launcher. */
export type InstanceLoader = "vanilla" | "fabric" | "quilt" | "forge" | "neoforge";

export interface NewInstanceRequest {
  name: string;
  gameVersion: string;
  loader: InstanceLoader;
  /** Required for every loader but vanilla, `null` for vanilla. */
  loaderVersion: string | null;
}

export function createInstance(request: NewInstanceRequest): Promise<InstanceDto> {
  return call("create_instance", { ...request });
}

/** A loader's releases for a game version, newest first. */
export function listLoaderVersions(
  loader: Exclude<InstanceLoader, "vanilla">,
  gameVersion: string,
): Promise<LoaderReleaseDto[]> {
  return call("list_loader_versions", { loader, gameVersion });
}

export function installInstance(
  instanceId: string,
  onProgress: (event: ProgressEvent) => void,
): RunningOperation<InstallSummaryDto> {
  return startOperation("install_instance", (operationId) =>
    call("install_instance", { operationId, instanceId, onProgress: channel(onProgress) }),
  );
}

/** Starts the game; resolves when it exits. Output arrives through `onLog`. */
export function launchInstance(
  instanceId: string,
  onLog: (line: LogLineDto) => void,
): Promise<LaunchOutcomeDto> {
  if (!isTauri()) return Promise.reject(unavailable("launch_instance"));
  return call("launch_instance", { instanceId, onLog: channel(onLog) });
}

export function killInstance(instanceId: string): Promise<boolean> {
  return call("kill_instance", { instanceId });
}

export interface ModpackSearch {
  query: string;
  gameVersion: string | null;
  loader: Exclude<InstanceLoader, "vanilla"> | null;
  /** Zero-based. */
  page: number;
  pageSize: number;
}

/** One page of Modrinth modpacks. */
export function searchModpacks(search: ModpackSearch): Promise<ModpackPageDto> {
  return call("search_modpacks", { ...search });
}

/** A modpack and its versions, newest first. */
export function getModpack(project: string): Promise<ModpackDetailsDto> {
  return call("get_modpack", { project });
}

/** Installs a Modrinth modpack version into a new instance, game and loader included. */
export function installModpack(
  project: string,
  version: string,
  onProgress: (event: ProgressEvent) => void,
): RunningOperation<ModpackInstallDto> {
  return startOperation("install_modpack", (operationId) =>
    call("install_modpack", { operationId, project, version, onProgress: channel(onProgress) }),
  );
}

/** Asks for a `.mrpack` file and installs it; resolves `null` when the picker is closed. */
export function importMrpack(
  onProgress: (event: ProgressEvent) => void,
): RunningOperation<ModpackInstallDto | null> {
  return startOperation("import_mrpack", (operationId) =>
    call("import_mrpack", { operationId, onProgress: channel(onProgress) }),
  );
}

/** Updates a modpack instance to its newest version, keeping the player's changes. */
export function updateModpack(
  instanceId: string,
  onProgress: (event: ProgressEvent) => void,
): RunningOperation<ModpackUpdateDto> {
  return startOperation("update_modpack", (operationId) =>
    call("update_modpack", { operationId, instanceId, onProgress: channel(onProgress) }),
  );
}

/** Asks where to save and exports the instance; resolves `null` when the picker is closed. */
export function exportMrpack(instanceId: string): Promise<ModpackExportDto | null> {
  return call("export_mrpack", { instanceId });
}

/** Subscribes to an app-wide event; resolves to the unsubscribe function. */
export async function onEvent<E extends keyof Events>(
  event: E,
  handler: (payload: Events[E]) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<Events[E]>(event, ({ payload }) => handler(payload));
}

export function getServerConfig(): Promise<ServerConfigDto> {
  return call("get_server_config");
}

export function getServerStatus(): Promise<ServerStatusDto> {
  return call("get_server_status");
}

export interface PlayOnCinepolOptions {
  onProgress: (event: ProgressEvent) => void;
  onLog: (line: LogLineDto) => void;
}

/** Prepares, installs and launches the CINEPOL instance; `cancel` stops either phase. */
export function playOnCinepol({
  onProgress,
  onLog,
}: PlayOnCinepolOptions): RunningOperation<LaunchOutcomeDto> {
  return startOperation("play_on_cinepol", (operationId) =>
    call("play_on_cinepol", {
      operationId,
      onProgress: channel(onProgress),
      onLog: channel(onLog),
    }),
  );
}

export function getSettings(): Promise<SettingsDto> {
  return call("get_settings");
}

/** `null` restores the default URL. */
export function setServerConfigUrl(url: string | null): Promise<SettingsDto> {
  return call("set_server_config_url", { url });
}

/** Opens a community link in the system browser (the capability limits the domains). */
export async function openExternal(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  try {
    await openUrl(url);
  } catch (error) {
    throw toIpcError(error);
  }
}
