import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import { Readout } from "../../components/ui/Readout";
import { asCatalog, type ModpackSummaryDto } from "../../ipc/client";

/**
 * A modpack in the catalog. The title is the link and covers the whole card,
 * so the card opens the pack without reading as one long link.
 */
export function ModpackCard({ modpack }: { modpack: ModpackSummaryDto }) {
  const { t, i18n } = useTranslation();
  const numbers = new Intl.NumberFormat(i18n.language);

  return (
    <article className="relative flex h-full flex-col gap-4 rounded-control border border-hairline bg-chassis p-5 transition-colors duration-[160ms] ease-settle focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-silk hover:border-hairline-lit">
      <div className="flex items-start gap-4">
        {modpack.iconUrl ? (
          <img
            src={modpack.iconUrl}
            alt=""
            width={56}
            height={56}
            loading="lazy"
            className="size-14 shrink-0 rounded-plate border border-hairline object-cover"
          />
        ) : (
          <span aria-hidden className="size-14 shrink-0 rounded-plate border border-hairline" />
        )}
        <h2 className="m-0 min-w-0 text-[1.0625rem] leading-snug font-semibold text-silk">
          <Link
            to="/modpacks/$source/$project"
            params={{ source: asCatalog(modpack.source), project: modpack.project }}
            className="text-silk no-underline outline-none after:absolute after:inset-0 after:content-['']"
          >
            {modpack.title}
          </Link>
        </h2>
      </div>
      <p className="m-0 line-clamp-2 text-[0.875rem] leading-[1.45] text-silk-dim">
        {modpack.description}
      </p>
      <div className="mt-auto flex flex-wrap items-end justify-between gap-3">
        <ul className="m-0 flex list-none flex-wrap gap-2 p-0">
          {modpack.loaders.map((loader) => (
            <li
              key={loader}
              className="plate rounded-plate border border-hairline-lit px-2 py-1 text-silk-dim"
            >
              {t(`instances.loaders.${loader}`, { defaultValue: loader })}
            </li>
          ))}
        </ul>
        <Readout
          unit={t("modpacks.downloads")}
          value={modpack.downloads === null ? null : numbers.format(modpack.downloads)}
          size="sm"
        />
      </div>
    </article>
  );
}
