import { describe, expect, it } from "vitest";
import { i18n } from "../../i18n";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError } from "../../ipc/client";
import { modErrorText } from "./modErrorText";

const t = i18n.getFixedT("es");

describe("modErrorText", () => {
  it("says why an update failed when its previous version was kept aside", () => {
    const kept = new IpcError("mod_backup_kept", "failed", "/tmp/mods-1/sodium.jar", "network");
    expect(modErrorText(t, kept)).toBe(
      esCommon.mods.errors.backupKeptBecause
        .replace("{{reason}}", esCommon.errors.network)
        .replace("{{path}}", "/tmp/mods-1/sodium.jar"),
    );
  });

  it("still names where the copy is when no reason came with it", () => {
    const kept = new IpcError("mod_backup_kept", "failed", "/tmp/mods-1/sodium.jar");
    expect(modErrorText(t, kept)).toBe(
      esCommon.mods.errors.backupKept.replace("{{path}}", "/tmp/mods-1/sodium.jar"),
    );
  });
});
