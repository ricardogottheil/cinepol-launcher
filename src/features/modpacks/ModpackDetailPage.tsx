import { Link, useParams } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import { Readout } from "../../components/ui/Readout";
import { asCatalog } from "../../ipc/client";
import { ModpackJobStatus } from "./ModpackJobStatus";
import { ModpackVersions } from "./ModpackVersions";
import { useModpack } from "./useModpacks";

/** One modpack: what it is, where it comes from, and its versions to install. */
export function ModpackDetailPage() {
  const { t, i18n } = useTranslation();
  const params = useParams({ from: "/modpacks/$source/$project" });
  const source = asCatalog(params.source);
  const details = useModpack(source, params.project, true);
  const summary = details.data?.summary;
  const numbers = new Intl.NumberFormat(i18n.language);

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-8 px-gutter-lg py-10">
      <Link
        to="/modpacks"
        className="nav-label self-start text-engrave no-underline transition-colors duration-[160ms] ease-settle hover:text-silk"
      >
        {t("modpacks.back")}
      </Link>
      {summary ? (
        <header className="flex flex-wrap items-start gap-6">
          {summary.iconUrl ? (
            <img
              src={summary.iconUrl}
              alt=""
              width={96}
              height={96}
              className="size-24 shrink-0 rounded-plate border border-hairline object-cover"
            />
          ) : (
            <span aria-hidden className="size-24 shrink-0 rounded-plate border border-hairline" />
          )}
          <div className="flex min-w-0 flex-1 flex-col gap-3">
            <h1 className="display-lg m-0 break-words text-silk">{summary.title}</h1>
            <span className="plate text-engrave">{t(`modpacks.sources.${source}`)}</span>
            <p className="m-0 max-w-[68ch] text-silk-dim">{summary.description}</p>
          </div>
          <div className="flex flex-wrap items-end gap-8">
            <Readout
              unit={t("modpacks.downloads")}
              value={summary.downloads === null ? null : numbers.format(summary.downloads)}
              size="sm"
            />
            <Readout
              unit={t("modpacks.spec.loader")}
              value={
                summary.loaders.length === 0
                  ? null
                  : summary.loaders
                      .map((loader) => t(`instances.loaders.${loader}`, { defaultValue: loader }))
                      .join(", ")
              }
              size="sm"
            />
          </div>
        </header>
      ) : null}
      <ModpackJobStatus />
      <ModpackVersions source={source} details={details.data} error={details.error} />
    </div>
  );
}
