import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import {
  getModpack,
  getModpackCatalogs,
  type ModpackCatalog,
  type ModpackSearch,
  searchModpacks,
} from "../../ipc/client";

/** Results per page; Modrinth allows up to 100, the launcher asks for fewer. */
export const MODPACK_PAGE_SIZE = 20;

/** Typing pauses this long before a search, to stay well under Modrinth's rate limit. */
export const SEARCH_DEBOUNCE_MS = 300;

export function useModpackSearch(search: ModpackSearch) {
  return useQuery({
    queryKey: ["modpacks", search],
    queryFn: () => searchModpacks(search),
    placeholderData: keepPreviousData,
    staleTime: 5 * 60 * 1000,
  });
}

/** The catalogs this build can use; static for the whole session. */
export function useModpackCatalogs() {
  return useQuery({
    queryKey: ["modpack-catalogs"],
    queryFn: getModpackCatalogs,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export function useModpack(source: ModpackCatalog, project: string, enabled: boolean) {
  return useQuery({
    queryKey: ["modpack", source, project],
    queryFn: () => getModpack(source, project),
    enabled,
    staleTime: 5 * 60 * 1000,
  });
}

/** `value`, once it has stopped changing for `delayMs`. */
export function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);
  return settled;
}
