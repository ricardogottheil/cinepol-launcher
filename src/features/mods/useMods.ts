import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import {
  checkModUpdates,
  installMod,
  listInstanceMods,
  type ModSearch,
  type ProgressEvent,
  previewModInstall,
  removeMod,
  searchMods,
  setModEnabled,
  updateMod,
} from "../../ipc/client";

/** Results per page; Modrinth allows up to 100, the launcher asks for fewer. */
export const MOD_PAGE_SIZE = 20;

/** How long a catalog answer (search results, a full update check) is trusted. */
export const CATALOG_STALE_MS = 5 * 60 * 1000;

/** How long an update check that missed some mods is trusted. */
export const PARTIAL_CHECK_STALE_MS = 30 * 1000;

export const instanceModsKey = (instanceId: string) => ["instance-mods", instanceId] as const;
export const modUpdatesKey = (instanceId: string) => ["mod-updates", instanceId] as const;

/** One page of mods that run on the instance; off while `enabled` is false (Vanilla). */
export function useModSearch(search: ModSearch, enabled = true) {
  return useQuery({
    queryKey: ["mods", search],
    queryFn: () => searchMods(search),
    enabled,
    placeholderData: keepPreviousData,
    staleTime: CATALOG_STALE_MS,
  });
}

/** What installing `project` would write; off until a mod is picked. */
export function useModInstallPreview(instanceId: string, project: string | null) {
  return useQuery({
    queryKey: ["mod-install-preview", instanceId, project],
    queryFn: () => previewModInstall(instanceId, project ?? ""),
    enabled: project !== null,
    // The plan depends on what is installed right now.
    staleTime: 0,
  });
}

/** The jars in the instance's `mods/` folder. */
export function useInstanceMods(instanceId: string) {
  return useQuery({
    queryKey: instanceModsKey(instanceId),
    queryFn: () => listInstanceMods(instanceId),
  });
}

/** Newer compatible versions of the installed mods; asks Modrinth only when `enabled`. */
export function useModUpdates(instanceId: string, enabled = true) {
  return useQuery({
    queryKey: modUpdatesKey(instanceId),
    queryFn: () => checkModUpdates(instanceId),
    enabled,
    // A check that missed some mods is asked again sooner, but not on every
    // mount: the catalog is likely failing or throttling right then.
    staleTime: (query) =>
      query.state.data?.unchecked.length ? PARTIAL_CHECK_STALE_MS : CATALOG_STALE_MS,
  });
}

function useRefreshMods(instanceId: string) {
  const queryClient = useQueryClient();
  return () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: instanceModsKey(instanceId) }),
      queryClient.invalidateQueries({ queryKey: modUpdatesKey(instanceId) }),
    ]);
}

export function useSetModEnabled(instanceId: string) {
  const refresh = useRefreshMods(instanceId);
  return useMutation({
    mutationFn: ({ filename, enabled }: { filename: string; enabled: boolean }) =>
      setModEnabled(instanceId, filename, enabled),
    // A refusal can leave the folder changed too (an interrupted rename).
    onSettled: refresh,
  });
}

export function useRemoveMod(instanceId: string) {
  const refresh = useRefreshMods(instanceId);
  return useMutation({
    mutationFn: ({ filename, removeOrphans }: { filename: string; removeOrphans: boolean }) =>
      removeMod(instanceId, filename, removeOrphans),
    onSettled: refresh,
  });
}

export interface ModWrite {
  project: string;
  /** Replaces the installed version instead of adding the mod. */
  update?: boolean;
}

/** Installs or updates one mod at a time, with its progress and a way to cancel it. */
export function useWriteMod(instanceId: string) {
  const refresh = useRefreshMods(instanceId);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const cancelRef = useRef<(() => Promise<boolean>) | null>(null);
  const mutation = useMutation({
    mutationFn: async ({ project, update = false }: ModWrite) => {
      const operation = (update ? updateMod : installMod)(instanceId, project, setProgress);
      cancelRef.current = operation.cancel;
      try {
        return await operation.done;
      } finally {
        cancelRef.current = null;
        setProgress(null);
      }
    },
    onSettled: refresh,
  });
  const cancel = () => cancelRef.current?.() ?? Promise.resolve(false);
  return { ...mutation, progress, cancel };
}
