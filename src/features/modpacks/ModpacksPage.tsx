import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { SelectField } from "../../components/ui/SelectField";
import { TextField } from "../../components/ui/TextField";
import { type InstanceLoader, toIpcError } from "../../ipc/client";
import { ProgressMeter } from "../instances/ProgressMeter";
import { instancesKey } from "../instances/useInstances";
import { ModpackRow } from "./ModpackRow";
import { useModpackJobStore } from "./modpackJobStore";
import {
  MODPACK_PAGE_SIZE,
  SEARCH_DEBOUNCE_MS,
  useDebouncedValue,
  useModpackSearch,
} from "./useModpacks";

type ModdedLoader = Exclude<InstanceLoader, "vanilla">;

const LOADERS: ModdedLoader[] = ["fabric", "quilt", "forge", "neoforge"];

function isModdedLoader(value: string): value is ModdedLoader {
  return (LOADERS as string[]).includes(value);
}

export function ModpacksPage() {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [loader, setLoader] = useState<ModdedLoader | null>(null);
  const [page, setPage] = useState(0);
  const query = useDebouncedValue(text.trim(), SEARCH_DEBOUNCE_MS);
  const search = useModpackSearch({
    source: "modrinth",
    query,
    gameVersion: null,
    loader,
    page,
    pageSize: MODPACK_PAGE_SIZE,
  });
  const { job, error, installed, importFile, installPackwizUrl, importPackwizFile } =
    useModpackJobStore();
  const [packUrl, setPackUrl] = useState("");
  const busy = job.kind !== "idle";
  const queryClient = useQueryClient();
  useEffect(() => {
    if (installed) void queryClient.invalidateQueries({ queryKey: instancesKey });
  }, [installed, queryClient]);
  const items = search.data?.items ?? [];
  const total = search.data?.total ?? null;
  const pageCount = total === null ? null : Math.max(1, Math.ceil(total / MODPACK_PAGE_SIZE));
  const hasNext = pageCount === null ? items.length === MODPACK_PAGE_SIZE : page + 1 < pageCount;

  const loaderOptions = [
    { value: "", label: t("modpacks.anyLoader") },
    ...LOADERS.map((value) => ({ value, label: t(`instances.loaders.${value}`) })),
  ];

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <div className="flex flex-wrap items-end justify-between gap-6">
        <div className="flex flex-col gap-2">
          <h1 className="display-lg m-0 text-silk">{t("modpacks.title")}</h1>
          <span className="plate text-engrave">{t("modpacks.source")}</span>
        </div>
        <Button onClick={() => void importFile()} disabled={busy}>
          {t("modpacks.import")}
        </Button>
      </div>

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

      <p role="status" className="m-0 text-[0.875rem]">
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

      <section
        aria-label={t("modpacks.packwiz.title")}
        className="flex flex-col gap-4 border-t border-hairline pt-6"
      >
        <span className="plate text-engrave">{t("modpacks.packwiz.source")}</span>
        <form
          className="flex flex-wrap items-end gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            void installPackwizUrl(packUrl.trim());
          }}
        >
          <TextField
            label={t("modpacks.packwiz.urlLabel")}
            hint={t("modpacks.packwiz.urlHint")}
            type="url"
            inputMode="url"
            placeholder="https://"
            value={packUrl}
            className="min-w-[320px] flex-1"
            onChange={(event) => setPackUrl(event.target.value)}
          />
          <Button type="submit" disabled={busy || packUrl.trim() === ""}>
            {t("modpacks.packwiz.install")}
          </Button>
          <Button onClick={() => void importPackwizFile()} disabled={busy}>
            {t("modpacks.packwiz.import")}
          </Button>
        </form>
      </section>

      <div className="flex flex-wrap items-start gap-4 border-t border-hairline pt-6">
        <TextField
          label={t("modpacks.searchLabel")}
          placeholder={t("modpacks.searchPlaceholder")}
          type="search"
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            setPage(0);
          }}
        />
        <SelectField
          label={t("modpacks.loaderLabel")}
          options={loaderOptions}
          value={loader ?? ""}
          onChange={(event) => {
            setLoader(isModdedLoader(event.target.value) ? event.target.value : null);
            setPage(0);
          }}
        />
      </div>

      <section aria-label={t("modpacks.results")} aria-busy={search.isFetching}>
        {search.isPending ? (
          <p className="m-0 text-engrave">{t("modpacks.loading")}</p>
        ) : search.isError ? (
          <p className="m-0 text-stale">{t(`errors.${toIpcError(search.error).code}`)}</p>
        ) : items.length === 0 ? (
          <p className="m-0 border-t border-hairline pt-6 text-silk-dim">{t("modpacks.empty")}</p>
        ) : (
          <>
            <ul className="m-0 list-none border-t border-hairline p-0">
              {items.map((modpack) => (
                <ModpackRow key={modpack.project} modpack={modpack} />
              ))}
            </ul>
            {page > 0 || hasNext ? (
              <nav
                aria-label={t("modpacks.pages")}
                className="flex flex-wrap items-center gap-4 pt-6"
              >
                <Button
                  onClick={() => setPage(page - 1)}
                  disabled={page === 0 || search.isFetching}
                >
                  {t("modpacks.previous")}
                </Button>
                <span className="plate text-engrave">
                  {pageCount === null
                    ? t("modpacks.page", { page: page + 1 })
                    : t("modpacks.pageOf", { page: page + 1, count: pageCount })}
                </span>
                <Button onClick={() => setPage(page + 1)} disabled={!hasNext || search.isFetching}>
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
