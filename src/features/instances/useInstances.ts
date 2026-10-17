import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createInstance, listGameVersions, listInstances } from "../../ipc/client";

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
    mutationFn: ({ name, gameVersion }: { name: string; gameVersion: string }) =>
      createInstance(name, gameVersion),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: instancesKey }),
  });
}
