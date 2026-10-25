import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import type { InstanceDto } from "../../ipc/client";
import { useActivityStore } from "./activityStore";
import { CrashNotice } from "./CrashNotice";
import { ProgressMeter } from "./ProgressMeter";
import type { InstanceSession } from "./useInstanceSession";

/** Play, or the way out of what is running: cancel an install, stop the game. */
export function PlayControl({
  instance,
  session,
}: {
  instance: InstanceDto;
  session: InstanceSession;
}) {
  const { t } = useTranslation();
  const { play, stop } = useActivityStore();
  const { activity } = session;

  if (activity.kind === "installing") {
    return <Button onClick={() => void activity.cancel()}>{t("instances.cancel")}</Button>;
  }
  if (activity.kind === "running") {
    return <Button onClick={() => void stop(instance.id)}>{t("instances.stop")}</Button>;
  }
  return (
    <Button
      onClick={() =>
        void play(instance.id, {
          syncPack: session.packSource === "packwiz" && instance.updatePackOnLaunch,
        })
      }
      disabled={session.busy || session.waitingForFiles}
    >
      {t("instances.play")}
    </Button>
  );
}

/** The install stage and how far it got, while an install runs. */
export function InstallProgress({ session }: { session: InstanceSession }) {
  const { t } = useTranslation();
  const { activity } = session;
  if (activity.kind !== "installing") return null;
  return (
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
  );
}

/**
 * One line on what is going on or went wrong: an error, the game running, an
 * export under way. Finished updates and exports show as toasts instead. It
 * takes no room while there is nothing to say.
 */
export function InstanceStatus({
  session,
  pendingFilesHint = false,
}: {
  session: InstanceSession;
  /** Say why Play is off while files wait for a manual download. */
  pendingFilesHint?: boolean;
}) {
  const { t } = useTranslation();
  const { error, activity, lastExit } = session;
  return (
    <p role="status" className="m-0 text-[0.875rem] empty:hidden">
      {error ? (
        <span className="text-stale">{t(`errors.${error}`)}</span>
      ) : activity.kind === "running" ? (
        <span className="text-silk-dim">{t("instances.playing")}</span>
      ) : activity.kind === "exporting" ? (
        <span className="text-silk-dim">{t("instances.exporting")}</span>
      ) : lastExit?.crashReport && !lastExit.killed ? (
        <CrashNotice instanceId={lastExit.instanceId} report={lastExit.crashReport} />
      ) : lastExit && lastExit.exitCode !== 0 && !lastExit.killed ? (
        <span className="text-stale">
          {t("instances.exited", { code: lastExit.exitCode ?? "-" })}
        </span>
      ) : pendingFilesHint && session.waitingForFiles ? (
        <span className="text-hazard">{t("instances.manual.title")}</span>
      ) : null}
    </p>
  );
}
