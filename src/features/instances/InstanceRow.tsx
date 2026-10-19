import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { Readout } from "../../components/ui/Readout";
import type { InstanceDto } from "../../ipc/client";
import { CINEPOL_SESSION, useActivityStore, useInstanceActivity } from "./activityStore";
import { splitPlaytime } from "./formatPlaytime";
import { InstanceSettingsPanel } from "./InstanceSettingsPanel";
import { LogPanel } from "./LogPanel";
import { ManualDownloadsPanel } from "./ManualDownloadsPanel";
import { ProgressMeter } from "./ProgressMeter";
import { instancesKey, useManualDownloads } from "./useInstances";

export function InstanceRow({ instance }: { instance: InstanceDto }) {
  const { t } = useTranslation();
  const own = useInstanceActivity(instance.id);
  const cinepolSession = useInstanceActivity(CINEPOL_SESSION);
  // A Play on CINEPOL session started from Home runs this instance too.
  const { activity, logs, error, lastExit, notice } =
    instance.origin === "cinepolServer" && cinepolSession.activity.kind !== "idle"
      ? cinepolSession
      : own;
  const { install, play, stop, updatePack, exportPack } = useActivityStore();
  const packSource =
    instance.modpackSource === "modrinth" || instance.modpackSource === "packwiz"
      ? instance.modpackSource
      : null;
  const isPackwiz = packSource === "packwiz";
  const manual = useManualDownloads(instance.id, isPackwiz);
  const waitingForFiles = (manual.data?.remaining.length ?? 0) > 0;
  const busy = activity.kind !== "idle";
  const [settingsOpen, setSettingsOpen] = useState(false);
  const queryClient = useQueryClient();
  // An update may move the instance to another game or loader version.
  useEffect(() => {
    if (notice?.kind === "updated") void queryClient.invalidateQueries({ queryKey: instancesKey });
  }, [notice, queryClient]);
  const loader = instance.loaderVersion
    ? `${instance.loader} ${instance.loaderVersion}`
    : instance.loader;

  return (
    <li className="flex flex-col gap-5 border-b border-hairline py-6">
      <div className="flex flex-wrap items-center justify-between gap-6">
        <div className="flex flex-wrap items-end gap-x-10 gap-y-3">
          <h2 className="display-md m-0 min-w-[200px] text-silk">{instance.name}</h2>
          <Readout unit={t("instances.spec.version")} value={instance.gameVersion} size="sm" />
          <Readout unit={t("instances.spec.loader")} value={loader} size="sm" />
          <Readout
            unit={t("instances.spec.playtime")}
            value={t("instances.playtime", splitPlaytime(instance.playtimeSecs))}
            size="sm"
          />
        </div>
        <div className="flex flex-wrap gap-3">
          <Button aria-expanded={settingsOpen} onClick={() => setSettingsOpen(!settingsOpen)}>
            {settingsOpen ? t("instances.settings.close") : t("instances.settings.open")}
          </Button>
          {activity.kind === "installing" ? (
            <Button onClick={() => void activity.cancel()}>{t("instances.cancel")}</Button>
          ) : activity.kind === "running" ? (
            <Button onClick={() => void stop(instance.id)}>{t("instances.stop")}</Button>
          ) : (
            <>
              {packSource ? (
                <Button onClick={() => void updatePack(instance.id, packSource)} disabled={busy}>
                  {t("instances.update")}
                </Button>
              ) : null}
              <Button onClick={() => void exportPack(instance.id)} disabled={busy}>
                {t("instances.export")}
              </Button>
              <Button onClick={() => void install(instance.id)} disabled={busy}>
                {t("instances.install")}
              </Button>
              <Button
                onClick={() =>
                  void play(instance.id, { syncPack: isPackwiz && instance.updatePackOnLaunch })
                }
                disabled={busy || waitingForFiles}
              >
                {t("instances.play")}
              </Button>
            </>
          )}
        </div>
      </div>

      {activity.kind === "installing" ? (
        <div className="flex flex-col gap-2">
          <span className="plate text-engrave">
            {activity.stage ? t(`stages.${activity.stage}`) : t("stages.metadata")}
          </span>
          <ProgressMeter
            label={t("instances.install")}
            done={activity.done}
            total={activity.total}
            unit={activity.unit}
          />
        </div>
      ) : null}

      <p role="status" className="m-0 text-[0.875rem]">
        {error ? (
          <span className="text-stale">{t(`errors.${error}`)}</span>
        ) : activity.kind === "running" ? (
          <span className="text-silk-dim">{t("instances.playing")}</span>
        ) : activity.kind === "exporting" ? (
          <span className="text-silk-dim">{t("instances.exporting")}</span>
        ) : notice?.kind === "upToDate" ? (
          <span className="text-silk-dim">{t("instances.upToDate")}</span>
        ) : notice?.kind === "updated" ? (
          <span className="text-silk-dim">
            {notice.kept > 0
              ? t("instances.keptChanges", { count: notice.kept })
              : t("instances.updated")}
          </span>
        ) : notice?.kind === "exported" ? (
          <span className="break-all text-silk-dim">
            {t("instances.exported", { path: notice.path })}
          </span>
        ) : lastExit && lastExit.exitCode !== 0 && !lastExit.killed ? (
          <span className="text-stale">
            {t("instances.exited", { code: lastExit.exitCode ?? "-" })}
          </span>
        ) : null}
      </p>

      {isPackwiz && manual.data && (waitingForFiles || manual.data.resolved.length > 0) ? (
        <ManualDownloadsPanel
          downloads={manual.data}
          error={manual.error}
          checking={manual.isFetching}
          onCheck={() => void manual.refetch()}
        />
      ) : null}

      {settingsOpen ? <InstanceSettingsPanel instance={instance} busy={busy} /> : null}

      {activity.kind === "running" || logs.length > 0 ? <LogPanel lines={logs} /> : null}
    </li>
  );
}
