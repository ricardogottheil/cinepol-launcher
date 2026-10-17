import { QueryClient } from "@tanstack/react-query";
import { IpcError } from "../ipc/client";

/** Backend data cache. Errors the backend cannot recover by retrying are not retried. */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: (failureCount, error) =>
          !(
            error instanceof IpcError && ["ipc_unavailable", "invalid_input"].includes(error.code)
          ) && failureCount < 2,
        refetchOnWindowFocus: false,
      },
    },
  });
}
