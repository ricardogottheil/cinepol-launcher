import { useQuery } from "@tanstack/react-query";
import { getAppInfo } from "../../ipc/client";

/** Name and version of the running app; static for the whole session. */
export function useAppInfo() {
  return useQuery({
    queryKey: ["app-info"],
    queryFn: getAppInfo,
    staleTime: Number.POSITIVE_INFINITY,
  });
}
