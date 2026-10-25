import type { TFunction } from "i18next";
import { toIpcError } from "../../ipc/client";

/** What a failed mod command says, naming the mod when the backend does. */
export function modErrorText(t: TFunction, error: unknown): string {
  const { code, details, causeCode } = toIpcError(error);
  if (details) {
    switch (code) {
      case "mod_incompatible":
        return t("mods.errors.incompatibleWith", { mod: details });
      case "no_compatible_version":
        return t("mods.errors.noVersionFor", { mod: details });
      case "already_exists":
        return t("mods.errors.alreadyInstalled", { mod: details });
      case "mod_backup_kept":
        return causeCode
          ? t("mods.errors.backupKeptBecause", {
              reason: t(`errors.${causeCode}`),
              path: details,
            })
          : t("mods.errors.backupKept", { path: details });
    }
  }
  return t(`errors.${code}`);
}
