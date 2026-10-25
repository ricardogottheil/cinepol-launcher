import bundled from "../../../config/server-config.json";

/** One community server as `server-config.json` (schema 2) lists it. */
export interface CommunityServerEntry {
  name: string;
  address: string;
  port?: number;
}

/** The CINEPOL configuration bundled with the launcher; the remote copy wins when it loads. */
export interface ServerConfig {
  schemaVersion: number;
  name: string;
  servers: CommunityServerEntry[];
  versions: { min: string; max: string; recommended: string };
  links: { website: string; twitch: string; discord: string };
}

export const bundledServerConfig: ServerConfig = bundled;

/** The address players type, omitting the default port. */
export function displayAddress({ address, port }: CommunityServerEntry): string {
  return port === undefined || port === 25565 ? address : `${address}:${port}`;
}
