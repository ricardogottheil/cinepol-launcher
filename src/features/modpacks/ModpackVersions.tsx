import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { Readout } from "../../components/ui/Readout";
import { type ModpackCatalog, type ModpackDetailsDto, toIpcError } from "../../ipc/client";
import { formatBytes } from "../instances/formatBytes";
import { useModpackJobStore } from "./modpackJobStore";

/** Versions listed for a modpack; the newest ones come first. */
const SHOWN_VERSIONS = 12;

/** The installable versions of a pack, each with its spec and Install. */
export function ModpackVersions({
  source,
  details,
  error,
}: {
  source: ModpackCatalog;
  details: ModpackDetailsDto | undefined;
  error: unknown;
}) {
  const { t } = useTranslation();
  const heading = useId();
  const busy = useModpackJobStore((state) => state.job.kind !== "idle");
  const installVersion = useModpackJobStore((state) => state.installVersion);

  return (
    <section aria-labelledby={heading} className="flex flex-col gap-4">
      <h2 id={heading} className="display-md m-0 text-silk">
        {t("modpacks.versionsTitle")}
      </h2>
      {error ? (
        <p className="m-0 text-stale">{t(`errors.${toIpcError(error).code}`)}</p>
      ) : !details ? (
        <p className="m-0 text-engrave">{t("modpacks.versionsLoading")}</p>
      ) : details.versions.length === 0 ? (
        <p className="m-0 text-silk-dim">{t("modpacks.noVersions")}</p>
      ) : (
        <ul className="m-0 list-none border-t border-hairline p-0">
          {details.versions.slice(0, SHOWN_VERSIONS).map((version) => (
            <li
              key={version.id}
              className="flex flex-wrap items-end justify-between gap-6 border-b border-hairline py-4"
            >
              <div className="flex flex-wrap items-end gap-x-10 gap-y-3">
                <Readout
                  unit={t("modpacks.spec.version")}
                  value={version.versionNumber}
                  size="sm"
                />
                <Readout
                  unit={t("modpacks.spec.minecraft")}
                  value={version.gameVersions.join(", ")}
                  size="sm"
                />
                <Readout
                  unit={t("modpacks.spec.loader")}
                  value={version.loaders
                    .map((loader) => t(`instances.loaders.${loader}`, { defaultValue: loader }))
                    .join(", ")}
                  size="sm"
                />
                <Readout
                  unit={t("modpacks.spec.size")}
                  value={
                    version.size === null
                      ? null
                      : `${formatBytes(version.size)} ${t("units.megabytes")}`
                  }
                  size="sm"
                />
              </div>
              <div className="flex items-center gap-4">
                {version.installable ? null : (
                  <span className="plate text-engrave">
                    {/* Modrinth versions lack a .mrpack; CurseForge files may forbid downloads. */}
                    {source === "curseforge"
                      ? t("modpacks.notDownloadable")
                      : t("modpacks.notInstallable")}
                  </span>
                )}
                <Button
                  disabled={busy || !version.installable}
                  onClick={() =>
                    void installVersion(
                      source,
                      details.summary.project,
                      version.id,
                      details.summary.title,
                    )
                  }
                >
                  {t("modpacks.install")}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
