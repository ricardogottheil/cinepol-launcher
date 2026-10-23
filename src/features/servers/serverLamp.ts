import type { ServerStatus } from "../../components/ui/StatusLamp";
import { toIpcError } from "../../ipc/client";

/** The lamp a status query shows, the same reading as on Home. */
export function serverLamp(query: {
  isPending: boolean;
  isError: boolean;
  error: unknown;
}): ServerStatus {
  if (query.isPending) return "checking";
  if (query.isError) {
    return toIpcError(query.error).code === "server_unreachable" ? "offline" : "unavailable";
  }
  return "online";
}
