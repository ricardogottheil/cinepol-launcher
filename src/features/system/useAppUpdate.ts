import { useQuery } from "@tanstack/react-query";
import { checkAppUpdate } from "../../ipc/client";

const HOUR_MS = 60 * 60 * 1000;

/** Whether a newer launcher is available; checked on start and hourly after that. */
export function useAppUpdate() {
  return useQuery({
    queryKey: ["app-update"],
    queryFn: checkAppUpdate,
    staleTime: HOUR_MS,
    refetchInterval: HOUR_MS,
    retry: false,
  });
}
