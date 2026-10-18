import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createInstance,
  type InstanceLoader,
  listGameVersions,
  listInstances,
  listLoaderVersions,
  type NewInstanceRequest,
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
