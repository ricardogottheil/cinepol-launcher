import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { type InstanceDto, toIpcError } from "../../ipc/client";
import { ProgressMeter } from "../instances/ProgressMeter";
import { InstalledMods } from "./InstalledMods";
import { ModBrowser } from "./ModBrowser";
import { modErrorText } from "./modErrorText";
import { useWriteMod } from "./useMods";

/** The mod being installed or updated, as the progress line names it. */
export interface ModJob {
  project: string;
  title: string;
  update: boolean;
}

/**
 * One mod install or update at a time for an instance. The instance page owns
 * it, so the job and its progress outlive switching tabs.
 */
export function useModJob(instanceId: string) {
  const [job, setJob] = useState<ModJob | null>(null);
  const writer = useWriteMod(instanceId);
  function start(next: ModJob) {
    setJob(next);
    writer.mutate({ project: next.project, update: next.update, title: next.title });
  }
  return { job, writer, start };
}

export type ModJobState = ReturnType<typeof useModJob>;

/**
 * An instance's mods: the installed list, or the Modrinth search filtered to
 * the instance's game version and loader.
 */
export function ModsPanel({
  instance,
  busy,
  jobs,
}: {
  instance: InstanceDto;
  busy: boolean;
  jobs: ModJobState;
}) {
  const { t } = useTranslation();
  const [browsing, setBrowsing] = useState(false);
  const { writer, start } = jobs;

  if (instance.loader === "vanilla") {
    return (
      <section aria-label={t("mods.title")} className="flex flex-col gap-4">
        <p className="m-0 max-w-[65ch] text-silk-dim">{t("mods.vanilla")}</p>
      </section>
    );
  }

  const locked = busy || writer.isPending;

  return (
    <section aria-label={t("mods.title")} className="flex flex-col gap-6">
      {browsing ? (
        <ModBrowser
          instance={instance}
          locked={locked}
          onInstall={(project, title) => start({ project, title, update: false })}
          onDone={() => setBrowsing(false)}
        />
      ) : (
        <InstalledMods
          instanceId={instance.id}
          locked={locked}
          onAdd={() => setBrowsing(true)}
          onUpdate={(project, title) => start({ project, title, update: true })}
        />
      )}
    </section>
  );
}

/** The running mod job's progress and Cancel, or how the last one ended. */
export function ModJobStatus({ jobs }: { jobs: ModJobState }) {
  const { t } = useTranslation();
  const { job, writer } = jobs;
  if (!job) return null;
  if (writer.isPending) {
    const progress = writer.progress;
    return (
      <div className="flex flex-wrap items-end gap-4">
        <div className="flex min-w-[240px] flex-1 flex-col gap-2">
          <span className="plate text-engrave">
            {t(job.update ? "mods.updating" : "mods.installing", { mod: job.title })}
          </span>
          <ProgressMeter
            label={t("mods.progress")}
            done={progress?.done ?? 0}
            total={progress?.total ?? null}
            unit={progress?.unit ?? "steps"}
          />
        </div>
        <Button onClick={() => void writer.cancel()}>{t("mods.cancelInstall")}</Button>
      </div>
    );
  }
  if (writer.isError) {
    // Cancelling is the player's own choice, not a failure to report.
    if (toIpcError(writer.error).code === "cancelled") return null;
    return (
      <p role="alert" className="m-0 text-[0.875rem] text-stale">
        {modErrorText(t, writer.error)}
      </p>
    );
  }
  return null;
}
