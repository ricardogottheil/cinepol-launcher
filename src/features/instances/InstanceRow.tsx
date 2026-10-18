import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { Readout } from "../../components/ui/Readout";
import type { InstanceDto } from "../../ipc/client";
import { CINEPOL_SESSION, useActivityStore, useInstanceActivity } from "./activityStore";
import { splitPlaytime } from "./formatPlaytime";
import { LogPanel } from "./LogPanel";
import { ProgressMeter } from "./ProgressMeter";

export function InstanceRow({ instance }: { instance: InstanceDto }) {
  const { t } = useTranslation();
  const own = useInstanceActivity(instance.id);
  const cinepolSession = useInstanceActivity(CINEPOL_SESSION);
  // A Play on CINEPOL session started from Home runs this instance too.
  const { activity, logs, error, lastExit } =
    instance.origin === "cinepolServer" && cinepolSession.activity.kind !== "idle"
      ? cinepolSession
      : own;
  const { install, play, stop } = useActivityStore();
  const busy = activity.kind !== "idle";
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
        <div className="flex gap-3">
          {activity.kind === "installing" ? (
            <Button onClick={() => void activity.cancel()}>{t("instances.cancel")}</Button>
          ) : activity.kind === "running" ? (
            <Button onClick={() => void stop(instance.id)}>{t("instances.stop")}</Button>
          ) : (
            <>
              <Button onClick={() => void install(instance.id)} disabled={busy}>
                {t("instances.install")}
              </Button>
              <Button onClick={() => void play(instance.id)} disabled={busy}>
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
        ) : lastExit && lastExit.exitCode !== 0 && !lastExit.killed ? (
          <span className="text-stale">
            {t("instances.exited", { code: lastExit.exitCode ?? "-" })}
          </span>
        ) : null}
      </p>

      {activity.kind === "running" || logs.length > 0 ? <LogPanel lines={logs} /> : null}
    </li>
  );
}
