import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { getModpack, type ModpackSearch, searchModpacks } from "../../ipc/client";

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

export function useModpack(project: string, enabled: boolean) {
  return useQuery({
    queryKey: ["modpack", project],
    queryFn: () => getModpack(project),
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
