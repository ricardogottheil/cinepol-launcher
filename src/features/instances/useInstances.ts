import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  checkManualDownloads,
  createInstance,
  deleteInstance,
  duplicateInstance,
  editServerList,
  getInstanceSettings,
  type InstanceLoader,
  type InstanceSettingsDto,
  listGameVersions,
  listInstances,
  listLoaderVersions,
  listServers,
  type NewInstanceRequest,
  openInstanceFolder,
  type ServerEditDto,
  updateInstanceSettings,
} from "../../ipc/client";

export const instancesKey = ["instances"] as const;

export function useInstances() {
  return useQuery({ queryKey: instancesKey, queryFn: listInstances });
}

export function useGameVersions(includeSnapshots: boolean) {
  return useQuery({
    queryKey: ["game-versions", includeSnapshots],
    queryFn: () => listGameVersions(includeSnapshots),
    staleTime: 10 * 60 * 1000,
  });
}

export function useCreateInstance() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: NewInstanceRequest) => createInstance(request),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: instancesKey }),
  });
}

/** A loader's releases for a game version; nothing to list for vanilla. */
export function useLoaderVersions(loader: InstanceLoader, gameVersion: string) {
  return useQuery({
    queryKey: ["loader-versions", loader, gameVersion],
    queryFn: () =>
      loader === "vanilla" ? Promise.resolve([]) : listLoaderVersions(loader, gameVersion),
    enabled: loader !== "vanilla" && gameVersion !== "",
    staleTime: 10 * 60 * 1000,
  });
}

const instanceSettingsKey = (instanceId: string) => ["instance-settings", instanceId] as const;

export function useInstanceSettings(instanceId: string) {
  return useQuery({
    queryKey: instanceSettingsKey(instanceId),
    queryFn: () => getInstanceSettings(instanceId),
  });
}

export function useUpdateInstanceSettings(instanceId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: InstanceSettingsDto) => updateInstanceSettings(instanceId, settings),
    onSuccess: (saved) => {
      queryClient.setQueryData(instanceSettingsKey(instanceId), saved);
      return queryClient.invalidateQueries({ queryKey: instancesKey });
    },
  });
}

export function useDuplicateInstance(instanceId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => duplicateInstance(instanceId, name),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: instancesKey }),
  });
}

export function useDeleteInstance(instanceId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => deleteInstance(instanceId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: instancesKey }),
  });
}

/** Opens the instance's game folder in the system's file manager. */
export function useOpenInstanceFolder(instanceId: string) {
  return useMutation({ mutationFn: () => openInstanceFolder(instanceId) });
}

/** While files wait for a manual download, the Downloads folder is checked this often. */
export const MANUAL_DOWNLOADS_POLL_MS = 3000;

export const manualDownloadsKey = (instanceId: string) => ["manual-downloads", instanceId] as const;

/** A packwiz instance's pending manual downloads; checking also installs the ones found. */
export function useManualDownloads(instanceId: string, enabled: boolean) {
  return useQuery({
    queryKey: manualDownloadsKey(instanceId),
    queryFn: () => checkManualDownloads(instanceId),
    enabled,
    // Keep checking while files are missing, and after a failed check.
    refetchInterval: (query) =>
      query.state.status === "error" || (query.state.data?.remaining.length ?? 0) > 0
        ? MANUAL_DOWNLOADS_POLL_MS
        : false,
  });
}

export const serversKey = (instanceId: string) => ["servers", instanceId] as const;

export function useServers(instanceId: string) {
  return useQuery({ queryKey: serversKey(instanceId), queryFn: () => listServers(instanceId) });
}

export function useEditServers(instanceId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (change: ServerEditDto) => editServerList(instanceId, change),
    onSuccess: (list) => queryClient.setQueryData(serversKey(instanceId), list),
  });
}
