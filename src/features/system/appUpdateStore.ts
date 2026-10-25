import { create } from "zustand";
import {
  type ClientErrorCode,
  installAppUpdate,
  type ProgressEvent,
  toIpcError,
} from "../../ipc/client";

interface AppUpdateInstall {
  installing: boolean;
  progress: ProgressEvent | null;
  error: ClientErrorCode | null;
  /**
   * Installs `version` unless an install already runs. Resolves with `true`
   * when the release changed under it, so the caller checks again.
   */
  install: (version: string) => Promise<boolean>;
  /** Forgets a refused install, so it does not greet the next visit. */
  clearError: () => void;
}

/**
 * The launcher update in progress, kept outside the update page so leaving
 * and coming back neither loses its progress nor offers a second install.
 */
export const useAppUpdateInstall = create<AppUpdateInstall>((set, get) => ({
  installing: false,
  progress: null,
  error: null,
  clearError: () => set({ error: null }),
  install: async (version) => {
    if (get().installing) return false;
    set({ installing: true, progress: null, error: null });
    try {
      // On success the launcher restarts, so nothing runs after this.
      await installAppUpdate(version, (progress) => set({ progress })).done;
      return false;
    } catch (failure) {
      const code = toIpcError(failure).code;
      if (code === "update_changed") return true;
      set({ error: code });
      return false;
    } finally {
      set({ installing: false, progress: null });
    }
  },
}));
