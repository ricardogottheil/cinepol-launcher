import { create } from "zustand";
import type { InstanceLoader, ModpackCatalog } from "../../ipc/client";

export type ModpackTab = "explore" | "installed" | "import";
export type ModdedLoader = Exclude<InstanceLoader, "vanilla">;

export interface ModpackBrowse {
  tab: ModpackTab;
  catalog: ModpackCatalog;
  text: string;
  loader: ModdedLoader | null;
  page: number;
}

export const initialBrowse: ModpackBrowse = {
  tab: "explore",
  catalog: "modrinth",
  text: "",
  loader: null,
  page: 0,
};

interface BrowseState extends ModpackBrowse {
  set: (change: Partial<ModpackBrowse>) => void;
}

/**
 * Where the player is in Modpacks, kept outside the page so opening a pack
 * and coming back keeps the tab, the catalog, the search and the page.
 */
export const useModpackBrowse = create<BrowseState>((set) => ({
  ...initialBrowse,
  set: (change) => set(change),
}));
