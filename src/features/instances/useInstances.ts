import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createInstance,
  deleteInstance,
  duplicateInstance,
  getInstanceSettings,
  type InstanceLoader,
  type InstanceSettingsDto,
  listGameVersions,
  listInstances,
  listLoaderVersions,
  type NewInstanceRequest,
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
