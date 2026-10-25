import { useQuery } from "@tanstack/react-query";
import { getServerConfig, IpcError, type ServerConfigDto } from "../../ipc/client";
import { bundledServerConfig, displayAddress } from "./serverConfig";

export const serverConfigKey = ["server-config"] as const;

/** Refresh period of every server's live status. */
export const STATUS_REFRESH_MS = 30_000;

/** The bundled config as the backend would report it, for previews and tests. */
export function bundledConfigDto(): ServerConfigDto {
  const config = bundledServerConfig;
  return {
    name: config.name,
    servers: config.servers.map((server) => ({
      name: server.name,
      address: displayAddress(server),
    })),
    minVersion: config.versions.min,
    maxVersion: config.versions.max,
    recommendedVersion: config.versions.recommended,
    links: config.links,
    origin: "bundled",
  };
}

export function useServerConfig() {
  return useQuery({
    queryKey: serverConfigKey,
    queryFn: async () => {
      try {
        return await getServerConfig();
      } catch (error) {
        // Outside the desktop app (previews, tests) the bundled copy stands in.
        if (error instanceof IpcError && error.code === "ipc_unavailable")
          return bundledConfigDto();
        throw error;
      }
    },
    placeholderData: bundledConfigDto,
    staleTime: 5 * 60 * 1000,
  });
}
