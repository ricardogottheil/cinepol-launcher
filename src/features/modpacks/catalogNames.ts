import type { ModpackCatalog } from "../../ipc/client";

/** Catalog names are brands, never translated. */
export const CATALOG_NAMES: Record<ModpackCatalog, string> = {
  modrinth: "Modrinth",
  curseforge: "CurseForge",
};
