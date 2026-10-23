import { type KeyboardEvent, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/ui/cx";
import { type ModpackTab, useModpackBrowse } from "./browseStore";
import { ExploreTab } from "./ExploreTab";
import { ImportTab } from "./ImportTab";
import { InstalledTab } from "./InstalledTab";
import { ModpackJobStatus } from "./ModpackJobStatus";
import { useModpackCatalogs } from "./useModpacks";

const TABS: ModpackTab[] = ["explore", "installed", "import"];

/** Modpacks in three tabs: find one, the ones installed, and bring your own. */
export function ModpacksPage() {
  const { t } = useTranslation();
  const tab = useModpackBrowse((state) => state.tab);
  const set = useModpackBrowse((state) => state.set);
  const catalogs = useModpackCatalogs();
  const hasCurseForge = catalogs.data?.curseforge === true;
  const ids = useId();
  const list = useRef<HTMLDivElement>(null);

  function onKey(event: KeyboardEvent) {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
    const jump = { Home: 0, End: TABS.length - 1 }[event.key];
    if (step === undefined && jump === undefined) return;
    event.preventDefault();
    const index = jump ?? (TABS.indexOf(tab) + (step ?? 0) + TABS.length) % TABS.length;
    const next = TABS[index] ?? "explore";
    set({ tab: next });
    list.current?.querySelector<HTMLElement>(`#${CSS.escape(`${ids}-${next}`)}`)?.focus();
  }

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-8 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("modpacks.title")}</h1>
      <div
        ref={list}
        role="tablist"
        aria-label={t("modpacks.tabsLabel")}
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
              aria-controls={`${ids}-${value}-panel`}
              tabIndex={selected ? 0 : -1}
              onClick={() => set({ tab: value })}
              className={cx(
                "nav-label -mb-px h-11 cursor-pointer border-b-2 px-4 transition-colors duration-[160ms] ease-settle",
                selected
                  ? "border-silk text-silk"
                  : "border-transparent text-engrave hover:text-silk-dim",
              )}
            >
              {t(`modpacks.tabs.${value}`)}
            </button>
          );
        })}
      </div>

      <ModpackJobStatus />

      <div role="tabpanel" id={`${ids}-${tab}-panel`} aria-labelledby={`${ids}-${tab}`}>
        {tab === "explore" ? (
          <ExploreTab hasCurseForge={hasCurseForge} />
        ) : tab === "installed" ? (
          <InstalledTab />
        ) : (
          <ImportTab hasCurseForge={hasCurseForge} />
        )}
      </div>
    </div>
  );
}
