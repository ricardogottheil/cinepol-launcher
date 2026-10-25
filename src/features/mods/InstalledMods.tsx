import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { ConfirmDialog } from "../../components/ui/ConfirmDialog";
import { cx } from "../../components/ui/cx";
import { Readout } from "../../components/ui/Readout";
import { asCatalog, type ModEntryDto, type ModUpdateDto } from "../../ipc/client";
import { CATALOG_NAMES } from "../modpacks/catalogNames";
import { modErrorText } from "./modErrorText";
import { useInstanceMods, useModUpdates, useRemoveMod, useSetModEnabled } from "./useMods";

/**
 * The jars in `mods/` as rule-separated rows: what each one is, why it is
 * there, and its actions. Updates are only asked for on request, since each
 * check asks Modrinth once per mod.
 */
export function InstalledMods({
  instanceId,
  locked,
  onAdd,
  onUpdate,
}: {
  instanceId: string;
  /** Another change to the instance is running. */
  locked: boolean;
  onAdd: () => void;
  onUpdate: (project: string, title: string) => void;
}) {
  const { t } = useTranslation();
  const mods = useInstanceMods(instanceId);
  const [checking, setChecking] = useState(false);
  const updates = useModUpdates(instanceId, checking);
  const toggle = useSetModEnabled(instanceId);
  const remove = useRemoveMod(instanceId);
  const [removing, setRemoving] = useState<ModEntryDto | null>(null);
  const entries = mods.data ?? [];
  const updateFor = new Map<string, ModUpdateDto>(
    (updates.data?.updates ?? []).map((update) => [update.project, update]),
  );
  const writing = locked || toggle.isPending || remove.isPending;
  const failure = toggle.error ?? remove.error;

  function checkUpdates() {
    if (checking) void updates.refetch();
    else setChecking(true);
  }

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <Readout
          unit={t("mods.count")}
          value={mods.data ? String(entries.length) : null}
          size="sm"
        />
        <div className="flex flex-wrap gap-3">
          {entries.length > 0 ? (
            <Button onClick={checkUpdates} disabled={updates.isFetching}>
              {updates.isFetching ? t("mods.checkingUpdates") : t("mods.checkUpdates")}
            </Button>
          ) : null}
          <Button onClick={onAdd}>{t("mods.add")}</Button>
        </div>
      </div>

      <UpdateSummary updates={updates} checking={checking} />
      {failure ? (
        <p role="alert" className="m-0 text-[0.875rem] text-stale">
          {modErrorText(t, failure)}
        </p>
      ) : null}

      {mods.isPending ? (
        <p className="m-0 text-engrave">{t("mods.loading")}</p>
      ) : mods.isError ? (
        <p className="m-0 text-stale">{modErrorText(t, mods.error)}</p>
      ) : entries.length === 0 ? (
        <p className="m-0 max-w-[65ch] border-t border-hairline pt-5 text-silk-dim">
          {t("mods.empty")}
        </p>
      ) : (
        <ul
          aria-label={t("mods.installedLabel")}
          className="m-0 list-none divide-y divide-hairline border-y border-hairline p-0"
        >
          {entries.map((entry) => (
            <ModRow
              key={entry.filename}
              entry={entry}
              update={entry.project ? updateFor.get(entry.project) : undefined}
              disabled={writing}
              onToggle={() => toggle.mutate({ filename: entry.filename, enabled: !entry.enabled })}
              onUpdate={() => entry.project && onUpdate(entry.project, entry.title)}
              onRemove={() => {
                remove.reset();
                setRemoving(entry);
              }}
            />
          ))}
        </ul>
      )}

      {removing ? (
        <RemoveDialog
          entry={removing}
          onCancel={() => setRemoving(null)}
          onConfirm={(removeOrphans) => {
            remove.mutate({ filename: removing.filename, removeOrphans });
            setRemoving(null);
          }}
        />
      ) : null}
    </div>
  );
}

function UpdateSummary({
  updates,
  checking,
}: {
  updates: ReturnType<typeof useModUpdates>;
  checking: boolean;
}) {
  const { t } = useTranslation();
  if (!checking || updates.isPending) return null;
  if (updates.isError) {
    return (
      <p role="status" className="m-0 text-[0.875rem] text-stale">
        {modErrorText(t, updates.error)}
      </p>
    );
  }
  const { updates: found, unchecked } = updates.data;
  return (
    <div role="status" className="flex flex-col gap-1 text-[0.875rem]">
      {found.length > 0 ? (
        <span className="text-silk-dim">{t("mods.updatesFound", { count: found.length })}</span>
      ) : unchecked.length === 0 ? (
        <span className="text-silk-dim">{t("mods.noUpdates")}</span>
      ) : null}
      {unchecked.length > 0 ? (
        // Never shown as up to date: they may have an update too.
        <span className="text-stale">{t("mods.unchecked", { mods: unchecked.join(", ") })}</span>
      ) : null}
    </div>
  );
}

function ModRow({
  entry,
  update,
  disabled,
  onToggle,
  onUpdate,
  onRemove,
}: {
  entry: ModEntryDto;
  update: ModUpdateDto | undefined;
  disabled: boolean;
  onToggle: () => void;
  onUpdate: () => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const why =
    entry.origin === "unknown"
      ? t("mods.notOnModrinth")
      : !entry.requested && entry.requiredBy.length > 0
        ? t("mods.neededBy", { mods: entry.requiredBy.join(", ") })
        : null;

  return (
    <li className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3 py-3">
      <div className="flex min-w-0 flex-col gap-1">
        <span className="flex flex-wrap items-center gap-3">
          <span className={cx("break-words", entry.enabled ? "text-silk" : "text-silk-dim")}>
            {entry.title}
          </span>
          {entry.enabled ? null : (
            <span className="plate rounded-plate border border-hairline px-2 py-1 text-engrave">
              {t("mods.off")}
            </span>
          )}
        </span>
        <span className="flex flex-wrap items-baseline gap-x-2">
          <span className="readout-sm break-all text-engrave">
            {entry.versionNumber ?? entry.filename}
          </span>
          {entry.source ? (
            <span className="text-[0.875rem] text-silk-dim">
              · {CATALOG_NAMES[asCatalog(entry.source)]}
            </span>
          ) : null}
          {why ? <span className="text-[0.875rem] text-silk-dim">· {why}</span> : null}
        </span>
      </div>
      <div className="flex flex-wrap gap-2">
        {update ? (
          <Button onClick={onUpdate} disabled={disabled}>
            {t("mods.update", { version: update.versionNumber })}
          </Button>
        ) : null}
        <Button onClick={onToggle} disabled={disabled}>
          {entry.enabled ? t("mods.disable") : t("mods.enable")}
        </Button>
        <Button onClick={onRemove} disabled={disabled}>
          {t("mods.remove")}
        </Button>
      </div>
    </li>
  );
}

function RemoveDialog({
  entry,
  onCancel,
  onConfirm,
}: {
  entry: ModEntryDto;
  onCancel: () => void;
  onConfirm: (removeOrphans: boolean) => void;
}) {
  const { t } = useTranslation();
  const [withOrphans, setWithOrphans] = useState(true);
  return (
    <ConfirmDialog
      title={t("mods.removeTitle", { mod: entry.title })}
      confirmLabel={t("mods.remove")}
      cancelLabel={t("mods.cancel")}
      destructive
      onCancel={onCancel}
      onConfirm={() => onConfirm(entry.orphans.length > 0 && withOrphans)}
    >
      <div className="flex flex-col gap-3">
        <p className="m-0">{t("mods.removeBody")}</p>
        {entry.requiredBy.length > 0 ? (
          <p className="m-0 text-hazard">
            {t("mods.removeNeeded", { mods: entry.requiredBy.join(", ") })}
          </p>
        ) : null}
        {entry.orphans.length > 0 ? (
          <label className="flex cursor-pointer items-start gap-3">
            <input
              type="checkbox"
              checked={withOrphans}
              onChange={(event) => setWithOrphans(event.target.checked)}
              className="mt-1 size-4 shrink-0 accent-silk"
            />
            <span>{t("mods.removeOrphans", { mods: entry.orphans.join(", ") })}</span>
          </label>
        ) : null}
      </div>
    </ConfirmDialog>
  );
}
