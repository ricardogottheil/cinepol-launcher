import { useIsMutating, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import type { InstanceDto, ServerConfigDto } from "../../ipc/client";
import { modWriteKey } from "../mods/useMods";
import { useServerConfig } from "../server/useServer";
import { CINEPOL_SESSION, useInstanceActivity } from "./activityStore";
import { instancesKey, manualDownloadsKey, serversKey, useManualDownloads } from "./useInstances";

export type PackSource = "modrinth" | "curseforge" | "packwiz";

/**
 * What an instance is doing right now, shared by its row in the list and its
 * own page: the activity (a mod install counts too), the modpack it follows
 * and the files it still waits for. Mount it once per screen; it refreshes
 * the data the game or an update rewrites.
 */
/**
 * Whether Play on CINEPOL runs this instance: the community pack's instance
 * when the config names a pack, otherwise the managed Vanilla one.
 */
export function isCinepolInstance(
  instance: InstanceDto,
  config: Pick<ServerConfigDto, "pack"> | undefined,
): boolean {
  const pack = config?.pack;
  if (pack) return instance.modpackSource === "packwiz" && instance.modpackProject === pack;
  return instance.origin === "cinepolServer";
}

export function useInstanceSession(instance: InstanceDto) {
  const own = useInstanceActivity(instance.id);
  const cinepolSession = useInstanceActivity(CINEPOL_SESSION);
  const config = useServerConfig();
  // A Play on CINEPOL session started from Home runs this instance too.
  const session =
    isCinepolInstance(instance, config.data) && cinepolSession.activity.kind !== "idle"
      ? cinepolSession
      : own;
  const packSource: PackSource | null =
    instance.modpackSource === "modrinth" ||
    instance.modpackSource === "curseforge" ||
    instance.modpackSource === "packwiz"
      ? instance.modpackSource
      : null;
  // packwiz and CurseForge packs may hold files the player downloads by hand.
  const hasManualFiles = packSource === "packwiz" || packSource === "curseforge";
  const manual = useManualDownloads(instance.id, hasManualFiles);
  const waitingForFiles = (manual.data?.remaining.length ?? 0) > 0;
  // A mod install or update holds the instance like any other activity,
  // wherever it was started.
  const writingMods = useIsMutating({ mutationKey: modWriteKey(instance.id) }) > 0;

  const queryClient = useQueryClient();
  const { lastExit, updates } = session;
  // The game rewrites servers.dat when it exits.
  useEffect(() => {
    if (lastExit) void queryClient.invalidateQueries({ queryKey: serversKey(instance.id) });
  }, [lastExit, queryClient, instance.id]);

  // An update may move the instance to another game or loader version, and a
  // packwiz update may add files to download by hand.
  useEffect(() => {
    if (!updates) return;
    void queryClient.invalidateQueries({ queryKey: instancesKey });
    void queryClient.invalidateQueries({ queryKey: manualDownloadsKey(instance.id) });
  }, [updates, queryClient, instance.id]);

  return {
    ...session,
    busy: session.activity.kind !== "idle" || writingMods,
    packSource,
    hasManualFiles,
    manual,
    waitingForFiles,
  };
}

export type InstanceSession = ReturnType<typeof useInstanceSession>;
