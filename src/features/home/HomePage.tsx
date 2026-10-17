import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import serverBanner from "../../assets/worlds/cinepol-recursos.webp";
import { Button } from "../../components/ui/Button";
import { Meter } from "../../components/ui/Meter";
import { Readout } from "../../components/ui/Readout";
import { SignalControl } from "../../components/ui/SignalControl";
import { SpecSheet } from "../../components/ui/SpecSheet";
import { type ServerStatus, StatusLamp } from "../../components/ui/StatusLamp";
import { WorldFrame } from "../../components/ui/WorldFrame";
import { toIpcError } from "../../ipc/client";
import { useAccounts } from "../accounts/useAccounts";
import { CINEPOL_SESSION, useActivityStore, useInstanceActivity } from "../instances/activityStore";
import { splitPlaytime } from "../instances/formatPlaytime";
import { LogPanel } from "../instances/LogPanel";
import { useInstances } from "../instances/useInstances";
import { bundledConfigDto, useServerConfig, useServerStatus } from "../server/useServer";

const plateLink =
  "plate-lg inline-flex h-11 items-center rounded-plate border border-hairline-lit px-4 text-silk-dim no-underline transition-colors duration-[160ms] ease-settle hover:border-engrave hover:text-silk";

/** The player's instances, or an empty state. */
function HomeInstances() {
  const { t } = useTranslation();
  const instances = useInstances();
  if (!instances.data || instances.data.length === 0) {
    return (
      <p className="mt-6 mb-0 border-t border-hairline pt-6 text-silk-dim">
        {t("home.instances.empty")}
      </p>
    );
  }
  return (
    <ul className="mt-6 mb-0 list-none border-t border-hairline p-0">
      {instances.data.map((instance) => (
        <li
          key={instance.id}
          className="flex flex-wrap items-baseline justify-between gap-4 border-b border-hairline py-4"
        >
          <Link to="/instances" className="display-md text-silk no-underline hover:text-silk-dim">
            {instance.name}
          </Link>
          <span data-readout className="readout-sm text-engrave">
            {instance.gameVersion} · {t("instances.playtime", splitPlaytime(instance.playtimeSecs))}
          </span>
        </li>
      ))}
    </ul>
  );
}

/** The signal control and what follows a press: install progress, the running game, errors. */
function QuickPlay({ recommended }: { recommended: string }) {
  const { t } = useTranslation();
  const { activity, logs, error, lastExit } = useInstanceActivity(CINEPOL_SESSION);
  const playCinepol = useActivityStore((state) => state.playCinepol);

  return (
    <div data-enter-control className="mt-8 flex flex-col gap-4">
      {activity.kind === "idle" ? (
        <SignalControl
          label={t("home.play")}
          detail={t("home.gameVersion", { version: recommended })}
          onPress={() => void playCinepol()}
        />
      ) : (
        <div className="flex flex-col gap-3 rounded-control border border-hairline-lit p-4">
          <div className="flex items-center justify-between gap-4">
            <span className="plate text-silk">
              {activity.kind === "running"
                ? t("home.session.playing")
                : t(`stages.${activity.stage ?? "metadata"}`)}
            </span>
            <Button onClick={() => void activity.cancel?.()} disabled={!activity.cancel}>
              {activity.kind === "running" ? t("instances.stop") : t("instances.cancel")}
            </Button>
          </div>
          {activity.kind === "installing" ? (
            <Meter label={t("home.play")} value={activity.done} max={activity.total} />
          ) : null}
        </div>
      )}
      <p role="status" className="m-0 text-[0.875rem]">
        {error ? (
          <span className="text-stale">{t(`errors.${error}`)}</span>
        ) : lastExit && lastExit.exitCode !== 0 && !lastExit.killed ? (
          <span className="text-stale">
            {t("instances.exited", { code: lastExit.exitCode ?? "-" })}
          </span>
        ) : null}
      </p>
      {activity.kind === "running" || logs.length > 0 ? <LogPanel lines={logs} /> : null}
    </div>
  );
}

/** Launcher home: quick play on CINEPOL with its live status, then the player's instances. */
export function HomePage() {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const accountName =
    accounts.data?.accounts.find((account) => account.id === accounts.data?.activeId)?.username ??
    null;
  const config = useServerConfig().data ?? bundledConfigDto();
  const statusQuery = useServerStatus();
  const status: ServerStatus = statusQuery.isPending
    ? "checking"
    : statusQuery.isError
      ? toIpcError(statusQuery.error).code === "server_unreachable"
        ? "offline"
        : "unavailable"
      : "online";
  const live = statusQuery.data;

  return (
    <div className="@container mx-auto flex max-w-[1120px] flex-col px-gutter-lg">
      <section
        aria-labelledby="quick-play-title"
        className="grid grid-cols-12 items-start gap-x-10 gap-y-8 border-b border-hairline py-10"
      >
        <div className="col-span-12 flex flex-col @3xl:col-span-7">
          <h1 id="quick-play-title" className="display-lg m-0 text-silk">
            {t("home.title", { server: config.name })}
          </h1>

          <div className="mt-8 flex flex-wrap items-start gap-x-12 gap-y-6">
            <SpecSheet
              rows={[
                { label: t("home.spec.server"), value: config.address },
                {
                  label: t("home.spec.version"),
                  value: t("home.recommended", { version: config.recommendedVersion }),
                },
              ]}
            />
            <SpecSheet
              rows={[{ label: t("home.spec.account"), value: accountName ?? t("home.noAccount") }]}
            />
          </div>

          <QuickPlay recommended={config.recommendedVersion} />
          {accountName ? null : (
            <Link to="/accounts" className={`${plateLink} mt-3 self-start`}>
              {t("home.addAccount")}
            </Link>
          )}
        </div>

        <div className="col-span-12 flex flex-col gap-6 @3xl:col-span-5">
          <WorldFrame
            world="overworld"
            src={serverBanner}
            caption={config.name}
            className="hidden @3xl:block"
          />
          <div
            data-live-status
            data-status={status}
            className="flex flex-wrap items-start gap-x-10 gap-y-4"
          >
            <Readout
              unit={t("home.readouts.online")}
              value={live ? `${live.onlinePlayers} / ${live.maxPlayers}` : null}
              size="md"
              lead={<StatusLamp status={status} />}
            />
            <Readout
              unit={t("home.readouts.versions")}
              value={`${config.minVersion} – ${config.maxVersion}`}
              size="md"
            />
            {live?.latencyMs != null ? (
              <Readout unit={t("home.readouts.latency")} value={`${live.latencyMs} ms`} size="md" />
            ) : null}
          </div>
          {live ? (
            <p className="m-0 whitespace-pre-line text-[0.875rem] text-silk-dim">{live.motd}</p>
          ) : (
            <span data-readout className="readout-sm -mt-2 text-engrave">
              {t(`home.status.${status}` as "home.status.unavailable")}
            </span>
          )}
        </div>
      </section>

      <section aria-labelledby="instances-title" className="py-10">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <h2 id="instances-title" className="display-md m-0 text-silk">
            {t("home.instances.title")}
          </h2>
          <div className="flex gap-3">
            <Link to="/modpacks" className={plateLink}>
              {t("home.instances.import")}
            </Link>
            <Link to="/instances" className={plateLink}>
              {t("home.instances.create")}
            </Link>
          </div>
        </div>
        <HomeInstances />
      </section>
    </div>
  );
}
