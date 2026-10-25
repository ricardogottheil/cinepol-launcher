import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  listSavedServers,
  pingServer,
  removeSavedServer,
  type SavedServerFormDto,
  saveServer,
} from "../../ipc/client";
import { STATUS_REFRESH_MS } from "../server/useServer";

export const savedServersKey = ["saved-servers"] as const;

export function useSavedServers() {
  return useQuery({ queryKey: savedServersKey, queryFn: listSavedServers });
}

/** A server's live status, refreshed every STATUS_REFRESH_MS. */
export function useServerPing(address: string) {
  return useQuery({
    queryKey: ["server-status", address],
    queryFn: () => pingServer(address),
    refetchInterval: STATUS_REFRESH_MS,
    retry: false,
  });
}

export function useSaveServer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (form: SavedServerFormDto) => saveServer(form),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: savedServersKey }),
  });
}

export function useRemoveSavedServer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (serverId: string) => removeSavedServer(serverId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: savedServersKey }),
  });
}
