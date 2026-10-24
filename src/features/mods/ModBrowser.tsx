import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { ConfirmDialog } from "../../components/ui/ConfirmDialog";
import { Readout } from "../../components/ui/Readout";
import { TextField } from "../../components/ui/TextField";
import type { InstanceDto, ModInstallPreviewDto, ModSummaryDto } from "../../ipc/client";
import { SEARCH_DEBOUNCE_MS, useDebouncedValue } from "../modpacks/useModpacks";
import { modErrorText } from "./modErrorText";
import { MOD_PAGE_SIZE, useInstanceMods, useModInstallPreview, useModSearch } from "./useMods";

/**
 * Modrinth mods that run on this instance: the filter is the instance's own
 * game version and loader, shown as a spec line, never a choice. Installing
 * first shows what comes with the mod.
 */
export function ModBrowser({
  instance,
  locked,
  onInstall,
  onDone,
}: {
  instance: InstanceDto;
  locked: boolean;
  onInstall: (project: string, title: string) => void;
  onDone: () => void;
}) {
  const { t, i18n } = useTranslation();
  const [text, setText] = useState("");
  const [page, setPage] = useState(0);
  const query = useDebouncedValue(text.trim(), SEARCH_DEBOUNCE_MS);
  const search = useModSearch({
    instanceId: instance.id,
    query,
    page,
    pageSize: MOD_PAGE_SIZE,
  });
  const installed = new Set(
    (useInstanceMods(instance.id).data ?? []).flatMap((entry) =>
      entry.project ? [entry.project] : [],
    ),
  );
  const [picked, setPicked] = useState<ModSummaryDto | null>(null);
  const preview = useModInstallPreview(instance.id, picked?.project ?? null);
  // Why a mod cannot be installed, kept on its row once the preview says so.
  const [refused, setRefused] = useState<Record<string, string>>({});
  const previewError = preview.error;

  useEffect(() => {
    if (!picked || !previewError) return;
    setRefused((current) => ({ ...current, [picked.project]: modErrorText(t, previewError) }));
    setPicked(null);
  }, [picked, previewError, t]);

  const items = search.data?.items ?? [];
  const total = search.data?.total ?? null;
  const pageCount = total === null ? null : Math.max(1, Math.ceil(total / MOD_PAGE_SIZE));
  const hasNext = pageCount === null ? items.length === MOD_PAGE_SIZE : page + 1 < pageCount;
  const numbers = new Intl.NumberFormat(i18n.language);
  const loader = t(`instances.loaders.${instance.loader}`, { defaultValue: instance.loader });

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <Readout unit={t("mods.for")} value={`${instance.gameVersion} · ${loader}`} size="sm" />
        <Button onClick={onDone}>{t("mods.doneAdding")}</Button>
      </div>
      <TextField
        label={t("mods.searchLabel")}
        placeholder={t("mods.searchPlaceholder")}
        type="search"
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          setPage(0);
        }}
      />

      <section aria-label={t("mods.results")} aria-busy={search.isFetching}>
        {search.isPending ? (
          <p className="m-0 text-engrave">{t("mods.searching")}</p>
        ) : search.isError ? (
          <p className="m-0 text-stale">{modErrorText(t, search.error)}</p>
        ) : items.length === 0 ? (
          <p className="m-0 border-t border-hairline pt-5 text-silk-dim">{t("mods.noResults")}</p>
        ) : (
          <>
            <ul className="m-0 list-none divide-y divide-hairline border-y border-hairline p-0">
              {items.map((mod) => (
                <li
                  key={mod.project}
                  className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3 py-3"
                >
                  <div className="flex min-w-0 flex-1 items-start gap-4">
                    {mod.iconUrl ? (
                      <img
                        src={mod.iconUrl}
                        alt=""
                        width={40}
                        height={40}
                        loading="lazy"
                        className="size-10 shrink-0 rounded-plate border border-hairline object-cover"
                      />
                    ) : (
                      <span
                        aria-hidden
                        className="size-10 shrink-0 rounded-plate border border-hairline"
                      />
                    )}
                    <div className="flex min-w-0 flex-col gap-1">
                      <span className="text-silk">{mod.title}</span>
                      <span className="line-clamp-2 max-w-[65ch] text-[0.875rem] leading-[1.45] text-silk-dim">
                        {mod.description}
                      </span>
                      {refused[mod.project] ? (
                        <span className="text-[0.875rem] text-stale">{refused[mod.project]}</span>
                      ) : null}
                    </div>
                  </div>
                  <div className="flex items-end gap-6">
                    <Readout
                      unit={t("mods.downloads")}
                      value={mod.downloads === null ? null : numbers.format(mod.downloads)}
                      size="sm"
                    />
                    <ResultAction
                      installed={installed.has(mod.project)}
                      refused={mod.project in refused}
                      checking={picked?.project === mod.project}
                      disabled={locked || picked !== null}
                      title={mod.title}
                      onInstall={() => setPicked(mod)}
                    />
                  </div>
                </li>
              ))}
            </ul>
            {page > 0 || hasNext ? (
              <nav aria-label={t("mods.pages")} className="flex flex-wrap items-center gap-4 pt-6">
                <Button
                  onClick={() => setPage(page - 1)}
                  disabled={page === 0 || search.isFetching}
                >
                  {t("mods.previous")}
                </Button>
                <span className="plate text-engrave">
                  {pageCount === null
                    ? t("mods.page", { page: page + 1 })
                    : t("mods.pageOf", { page: page + 1, count: pageCount })}
                </span>
                <Button onClick={() => setPage(page + 1)} disabled={!hasNext || search.isFetching}>
                  {t("mods.next")}
                </Button>
              </nav>
            ) : null}
          </>
        )}
      </section>

      {/* Only a plan read just now: what is installed may have changed. */}
      {picked && preview.data && !preview.isFetching ? (
        <InstallDialog
          mod={picked}
          instanceName={instance.name}
          preview={preview.data}
          onCancel={() => setPicked(null)}
          onConfirm={() => {
            onInstall(picked.project, picked.title);
            setPicked(null);
          }}
        />
      ) : null}
    </div>
  );
}

function ResultAction({
  installed,
  refused,
  checking,
  disabled,
  title,
  onInstall,
}: {
  installed: boolean;
  refused: boolean;
  checking: boolean;
  disabled: boolean;
  title: string;
  onInstall: () => void;
}) {
  const { t } = useTranslation();
  const plate = "plate inline-flex h-11 items-center rounded-plate border px-4";
  if (installed) {
    return <span className={`${plate} border-hairline text-engrave`}>{t("mods.installed")}</span>;
  }
  if (refused) {
    return <span className={`${plate} border-hairline text-stale`}>{t("mods.notCompatible")}</span>;
  }
  return (
    <Button
      onClick={onInstall}
      disabled={disabled}
      aria-label={checking ? undefined : `${t("mods.install")} ${title}`}
    >
      {checking ? t("mods.checking") : t("mods.install")}
    </Button>
  );
}

function InstallDialog({
  mod,
  instanceName,
  preview,
  onCancel,
  onConfirm,
}: {
  mod: ModSummaryDto;
  instanceName: string;
  preview: ModInstallPreviewDto;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  return (
    <ConfirmDialog
      title={t("mods.confirmTitle", { mod: mod.title })}
      confirmLabel={t("mods.confirmInstall", { count: preview.mods.length })}
      cancelLabel={t("mods.cancel")}
      onCancel={onCancel}
      onConfirm={onConfirm}
    >
      <div className="flex flex-col gap-3">
        <p className="m-0">{t("mods.confirmBody", { instance: instanceName })}</p>
        <ul className="m-0 list-none divide-y divide-hairline border-y border-hairline p-0">
          {preview.mods.map((planned) => (
            <li key={planned.project} className="flex flex-col gap-1 py-2">
              <span className="text-silk">{planned.title}</span>
              <span className="readout-sm text-engrave">
                {planned.versionNumber}
                {planned.requested ? null : (
                  <span className="text-silk-dim">
                    {" · "}
                    {t("mods.requiredBy", { mods: planned.requiredBy.join(", ") })}
                  </span>
                )}
              </span>
            </li>
          ))}
        </ul>
      </div>
    </ConfirmDialog>
  );
}
