import bundled from "../../../config/server-config.json";

/** CINEPOL server configuration. Milestone 6 adds the remote copy; this is the bundled fallback. */
export interface ServerConfig {
  schemaVersion: number;
  server: { name: string; address: string; port: number };
  versions: { min: string; max: string; recommended: string };
  links: { website: string; twitch: string; discord: string };
}

export const bundledServerConfig: ServerConfig = bundled;

/** The address players type, omitting the default port. */
export function displayAddress(config: ServerConfig): string {
  const { address, port } = config.server;
  return port === 25565 ? address : `${address}:${port}`;
}
