import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { SelectField } from "../../components/ui/SelectField";
import { TextField } from "../../components/ui/TextField";
import { type ModpackCatalog, toIpcError } from "../../ipc/client";
import { type ModdedLoader, useModpackBrowse } from "./browseStore";
import { ModpackCard } from "./ModpackCard";
import {
  MODPACK_PAGE_SIZE,
  SEARCH_DEBOUNCE_MS,
  useDebouncedValue,
  useModpackSearch,
} from "./useModpacks";

const LOADERS: ModdedLoader[] = ["fabric", "quilt", "forge", "neoforge"];

/** Catalog names are brands, never translated. */
export const CATALOG_NAMES: Record<ModpackCatalog, string> = {
  modrinth: "Modrinth",
  curseforge: "CurseForge",
};

function isModdedLoader(value: string): value is ModdedLoader {
  return (LOADERS as string[]).includes(value);
}

/** Search a catalog: filters on top, the packs as cards, then the pages. */
export function ExploreTab({ hasCurseForge }: { hasCurseForge: boolean }) {
  const { t } = useTranslation();
  const { catalog, text, loader, page, set } = useModpackBrowse();
  // Without a key CurseForge is not offered at all.
  const source: ModpackCatalog = hasCurseForge ? catalog : "modrinth";
  const catalogName = CATALOG_NAMES[source];
  const query = useDebouncedValue(text.trim(), SEARCH_DEBOUNCE_MS);
  const search = useModpackSearch({
    source,
    query,
    gameVersion: null,
    loader,
    page,
    pageSize: MODPACK_PAGE_SIZE,
  });
  const items = search.data?.items ?? [];
  const total = search.data?.total ?? null;
  const pageCount = total === null ? null : Math.max(1, Math.ceil(total / MODPACK_PAGE_SIZE));
  const hasNext = pageCount === null ? items.length === MODPACK_PAGE_SIZE : page + 1 < pageCount;

  const loaderOptions = [
    { value: "", label: t("modpacks.anyLoader") },
    ...LOADERS.map((value) => ({ value, label: t(`instances.loaders.${value}`) })),
  ];

  return (
    <div className="flex flex-col gap-8">
      <div className="flex flex-wrap items-start gap-4">
        {hasCurseForge ? (
          <SelectField
            label={t("modpacks.catalog")}
            options={(Object.keys(CATALOG_NAMES) as ModpackCatalog[]).map((value) => ({
              value,
              label: CATALOG_NAMES[value],
            }))}
            value={source}
            onChange={(event) =>
              set({
                catalog: event.target.value === "curseforge" ? "curseforge" : "modrinth",
                page: 0,
              })
            }
          />
        ) : null}
        <TextField
          label={t("modpacks.searchLabel", { catalog: catalogName })}
          placeholder={t("modpacks.searchPlaceholder")}
          type="search"
          value={text}
          className="min-w-[260px] flex-1"
          onChange={(event) => set({ text: event.target.value, page: 0 })}
        />
        <SelectField
          label={t("modpacks.loaderLabel")}
          options={loaderOptions}
          value={loader ?? ""}
          onChange={(event) =>
            set({
              loader: isModdedLoader(event.target.value) ? event.target.value : null,
              page: 0,
            })
          }
        />
      </div>

      <section aria-label={t("modpacks.results")} aria-busy={search.isFetching}>
        {search.isPending ? (
          <p className="m-0 text-engrave">{t("modpacks.loading", { catalog: catalogName })}</p>
        ) : search.isError ? (
          <p className="m-0 text-stale">{t(`errors.${toIpcError(search.error).code}`)}</p>
        ) : items.length === 0 ? (
          <p className="m-0 border-t border-hairline pt-6 text-silk-dim">{t("modpacks.empty")}</p>
        ) : (
          <>
            <ul className="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-4 p-0">
              {items.map((modpack) => (
                <li key={`${modpack.source}:${modpack.project}`}>
                  <ModpackCard modpack={modpack} />
                </li>
              ))}
            </ul>
            {page > 0 || hasNext ? (
              <nav
                aria-label={t("modpacks.pages")}
                className="flex flex-wrap items-center gap-4 pt-8"
              >
                <Button
                  onClick={() => set({ page: page - 1 })}
                  disabled={page === 0 || search.isFetching}
                >
                  {t("modpacks.previous")}
                </Button>
                <span className="plate text-engrave">
                  {pageCount === null
                    ? t("modpacks.page", { page: page + 1 })
                    : t("modpacks.pageOf", { page: page + 1, count: pageCount })}
                </span>
                <Button
                  onClick={() => set({ page: page + 1 })}
                  disabled={!hasNext || search.isFetching}
                >
                  {t("modpacks.next")}
                </Button>
              </nav>
            ) : null}
          </>
        )}
      </section>
    </div>
  );
}
