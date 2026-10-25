import { Link, useParams } from "@tanstack/react-router";
import { type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { cx } from "../../components/ui/cx";
import { Readout } from "../../components/ui/Readout";
import { type InstanceDto, toIpcError } from "../../ipc/client";
import { type ModJobState, ModJobStatus, ModsPanel, useModJob } from "../mods/ModsPanel";
import { useActivityStore } from "./activityStore";
import { splitPlaytime } from "./formatPlaytime";
import { InstallProgress, InstanceStatus, PlayControl } from "./InstanceActivity";
import { loaderLabel } from "./InstanceRow";
import { InstanceSettingsPanel } from "./InstanceSettingsPanel";
import { LogPanel } from "./LogPanel";
import { ManualDownloadsPanel } from "./ManualDownloadsPanel";
import { ServerListPanel } from "./ServerListPanel";
import { useInstanceSession } from "./useInstanceSession";
import { useInstances, useOpenInstanceFolder } from "./useInstances";

type InstanceTab = "servers" | "mods" | "settings" | "log";
const TABS: InstanceTab[] = ["servers", "mods", "settings", "log"];

/** One instance: its spec sheet and actions, then servers, mods, settings and log in tabs. */
export function InstanceDetailPage() {
  const { t } = useTranslation();
  const { instanceId } = useParams({ from: "/instances/$instanceId" });
  const instances = useInstances();
  const instance = instances.data?.find((candidate) => candidate.id === instanceId);

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-8 px-gutter-lg py-10">
      <Link
        to="/instances"
        className="nav-label self-start text-engrave no-underline transition-colors duration-[160ms] ease-settle hover:text-silk"
      >
        {t("instances.back")}
      </Link>
      {instances.isPending ? (
        <p className="m-0 text-engrave">{t("instances.loading")}</p>
      ) : instances.isError ? (
        <p className="m-0 text-stale">{t(`errors.${toIpcError(instances.error).code}`)}</p>
      ) : instance ? (
        // Another instance's page starts fresh: its own tab, no stale errors.
        <InstanceDetail key={instance.id} instance={instance} />
      ) : (
        <p className="m-0 text-silk-dim">{t("instances.notFound")}</p>
      )}
    </div>
  );
}

function InstanceDetail({ instance }: { instance: InstanceDto }) {
  const { t } = useTranslation();
  const session = useInstanceSession(instance);
  const { install, updatePack, exportPack } = useActivityStore();
  const openFolder = useOpenInstanceFolder(instance.id);
  const { activity, busy, packSource, manual } = session;
  const idle = activity.kind !== "installing" && activity.kind !== "running";
  const [tab, setTab] = useState<InstanceTab>("servers");
  const modJobs = useModJob(instance.id);
  // The game's output is what matters once it starts.
  const running = activity.kind === "running";
  useEffect(() => {
    if (running) setTab("log");
  }, [running]);

  return (
    <>
      <header className="flex flex-col gap-6">
        <h1 className="display-lg m-0 break-words text-silk">{instance.name}</h1>
        <div className="flex flex-wrap items-end justify-between gap-6">
          <div className="flex flex-wrap items-end gap-x-10 gap-y-3">
            <Readout unit={t("instances.spec.version")} value={instance.gameVersion} size="sm" />
            <Readout unit={t("instances.spec.loader")} value={loaderLabel(instance)} size="sm" />
            <Readout
              unit={t("instances.spec.playtime")}
              value={t("instances.playtime", splitPlaytime(instance.playtimeSecs))}
              size="sm"
            />
          </div>
          <div className="flex flex-wrap gap-3">
            <Button onClick={() => openFolder.mutate()} disabled={openFolder.isPending}>
              {t("instances.openFolder")}
            </Button>
            {idle ? (
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
              </>
            ) : null}
            <PlayControl instance={instance} session={session} />
          </div>
        </div>
        {openFolder.error ? (
          <p role="alert" className="m-0 text-[0.875rem] text-stale">
            {t(`errors.${toIpcError(openFolder.error).code}`)}
          </p>
        ) : null}
        <InstallProgress session={session} />
        <ModJobStatus jobs={modJobs} />
        <InstanceStatus session={session} />
      </header>

      {session.hasManualFiles &&
      manual.data &&
      (session.waitingForFiles || manual.data.resolved.length > 0) ? (
        <ManualDownloadsPanel
          downloads={manual.data}
          error={manual.error}
          checking={manual.isFetching}
          onCheck={() => void manual.refetch()}
        />
      ) : null}

      <InstanceTabs
        instance={instance}
        busy={busy}
        logs={session.logs}
        tab={tab}
        onTab={setTab}
        modJobs={modJobs}
      />
    </>
  );
}

function InstanceTabs({
  instance,
  busy,
  logs,
  tab,
  onTab: setTab,
  modJobs,
}: {
  instance: InstanceDto;
  busy: boolean;
  logs: Parameters<typeof LogPanel>[0]["lines"];
  tab: InstanceTab;
  onTab: (tab: InstanceTab) => void;
  modJobs: ModJobState;
}) {
  const { t } = useTranslation();
  const ids = useId();
  const list = useRef<HTMLDivElement>(null);

  function onKey(event: KeyboardEvent) {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
    const jump = { Home: 0, End: TABS.length - 1 }[event.key];
    if (step === undefined && jump === undefined) return;
    event.preventDefault();
    const index = jump ?? (TABS.indexOf(tab) + (step ?? 0) + TABS.length) % TABS.length;
    const next = TABS[index] ?? "servers";
    setTab(next);
    list.current?.querySelector<HTMLElement>(`#${CSS.escape(`${ids}-${next}`)}`)?.focus();
  }

  return (
    <div className="flex flex-col gap-6">
      <div
        ref={list}
        role="tablist"
        aria-label={t("instances.tabsLabel")}
        onKeyDown={onKey}
        className="flex gap-1 border-b border-hairline"
      >
        {TABS.map((value) => {
          const selected = value === tab;
          return (
            <button
              key={value}
              id={`${ids}-${value}`}
              type="button"
              role="tab"
              aria-selected={selected}
              // Only the selected tab's panel is rendered.
              aria-controls={selected ? `${ids}-${value}-panel` : undefined}
              tabIndex={selected ? 0 : -1}
              onClick={() => setTab(value)}
              className={cx(
                "nav-label -mb-px h-11 cursor-pointer border-b-2 px-4 transition-colors duration-[160ms] ease-settle",
                selected
                  ? "border-signal text-silk"
                  : "border-transparent text-engrave hover:text-silk-dim",
              )}
            >
              {t(`instances.tabs.${value}`)}
            </button>
          );
        })}
      </div>
      <div role="tabpanel" id={`${ids}-${tab}-panel`} aria-labelledby={`${ids}-${tab}`}>
        {tab === "servers" ? (
          <ServerListPanel instanceId={instance.id} />
        ) : tab === "mods" ? (
          <ModsPanel instance={instance} busy={busy} jobs={modJobs} />
        ) : tab === "settings" ? (
          <InstanceSettingsPanel instance={instance} busy={busy} />
        ) : (
          <LogPanel lines={logs} />
        )}
      </div>
    </div>
  );
}
