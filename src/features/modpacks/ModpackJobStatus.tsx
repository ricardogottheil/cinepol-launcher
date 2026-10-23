import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { ProgressMeter } from "../instances/ProgressMeter";
import { instancesKey } from "../instances/useInstances";
import { useModpackJobStore } from "./modpackJobStore";

/** The running install or import, and how the last one ended. */
export function ModpackJobStatus() {
  const { t } = useTranslation();
  const { job, error, installed } = useModpackJobStore();
  const queryClient = useQueryClient();
  useEffect(() => {
    if (installed) void queryClient.invalidateQueries({ queryKey: instancesKey });
  }, [installed, queryClient]);

  return (
    <>
      {job.kind === "installing" ? (
        <section
          aria-label={t("modpacks.progress")}
          className="flex flex-col gap-3 border-t border-hairline pt-6"
        >
          <div className="flex flex-wrap items-center justify-between gap-4">
            <span className="text-silk">
              {job.title ? t("modpacks.installing", { name: job.title }) : t("modpacks.importing")}
            </span>
            <Button onClick={() => void job.cancel()}>{t("modpacks.cancel")}</Button>
          </div>
          <span className="plate text-engrave">
            {job.stage ? t(`stages.${job.stage}`) : t("stages.metadata")}
          </span>
          <ProgressMeter
            label={t("modpacks.install")}
            done={job.done}
            total={job.total}
            unit={job.unit}
          />
        </section>
      ) : null}
      <p role="status" className="m-0 text-[0.875rem] empty:hidden">
        {error ? (
          <span className="text-stale">{t(`errors.${error}`)}</span>
        ) : installed ? (
          <span className="text-silk-dim">
            {installed.pending > 0
              ? t("modpacks.installedPending", {
                  name: installed.instance.name,
                  count: installed.pending,
                })
              : t("modpacks.installed", { name: installed.instance.name })}
          </span>
        ) : null}
      </p>
    </>
  );
}
