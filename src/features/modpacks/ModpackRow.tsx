import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { Readout } from "../../components/ui/Readout";
import { asCatalog, type ModpackSummaryDto, toIpcError } from "../../ipc/client";
import { formatBytes } from "../instances/formatBytes";
import { useModpackJobStore } from "./modpackJobStore";
import { useModpack } from "./useModpacks";

/** Versions listed when a modpack is opened; the newest ones come first. */
const SHOWN_VERSIONS = 8;

export function ModpackRow({ modpack }: { modpack: ModpackSummaryDto }) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const source = asCatalog(modpack.source);
  const details = useModpack(source, modpack.project, open);
  const busy = useModpackJobStore((state) => state.job.kind !== "idle");
  const installVersion = useModpackJobStore((state) => state.installVersion);
  const numbers = new Intl.NumberFormat(i18n.language);

  return (
    <li className="flex flex-col gap-5 border-b border-hairline py-6">
      <div className="flex flex-wrap items-center justify-between gap-6">
        <div className="flex min-w-0 flex-1 items-start gap-5">
          {modpack.iconUrl ? (
            <img
              src={modpack.iconUrl}
              alt=""
              width={48}
              height={48}
              loading="lazy"
              className="size-12 shrink-0 rounded-plate border border-hairline object-cover"
            />
          ) : (
            <span aria-hidden className="size-12 shrink-0 rounded-plate border border-hairline" />
          )}
          <div className="flex min-w-0 flex-col gap-2">
            <h2 className="display-md m-0 text-silk">{modpack.title}</h2>
            <p className="m-0 text-silk-dim">{modpack.description}</p>
          </div>
        </div>
        <div className="flex items-end gap-6">
          <Readout
            unit={t("modpacks.downloads")}
            value={modpack.downloads === null ? null : numbers.format(modpack.downloads)}
            size="sm"
          />
          <Button aria-expanded={open} onClick={() => setOpen(!open)}>
            {open ? t("modpacks.hideVersions") : t("modpacks.showVersions")}
          </Button>
        </div>
      </div>

      {open ? (
        details.isPending ? (
          <p className="m-0 text-engrave">{t("modpacks.versionsLoading")}</p>
        ) : details.isError ? (
          <p className="m-0 text-stale">{t(`errors.${toIpcError(details.error).code}`)}</p>
        ) : details.data.versions.length === 0 ? (
          <p className="m-0 text-silk-dim">{t("modpacks.noVersions")}</p>
        ) : (
          <ul className="m-0 list-none border-t border-hairline p-0">
            {details.data.versions.slice(0, SHOWN_VERSIONS).map((version) => (
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
                    <span className="plate text-engrave">{t("modpacks.notInstallable")}</span>
                  )}
                  <Button
                    disabled={busy || !version.installable}
                    onClick={() =>
                      void installVersion(source, modpack.project, version.id, modpack.title)
                    }
                  >
                    {t("modpacks.install")}
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )
      ) : null}
    </li>
  );
}
