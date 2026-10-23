import { type ChangeEvent, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, plateLinkClass } from "../../components/ui/Button";
import { CapeSwatch } from "../../components/ui/CapeSwatch";
import { cx } from "../../components/ui/cx";
import { toIpcError } from "../../ipc/client";
import { readSkinFile, type SkinFile } from "./skinFile";
import { useAccountCapes, useChangeSkin, useSetCape } from "./useAccounts";

export interface SkinDraft {
  file: SkinFile;
  slim: boolean;
}

/**
 * Picks a PNG, previews it on the 3D figure (the page owns the draft) and
 * applies it with one request. Nothing retries: Mojang may suspend accounts
 * that keep hitting its limit.
 */
export function SkinChanger({
  accountId,
  currentSlim,
  draft,
  onDraft,
}: {
  accountId: string;
  currentSlim: boolean;
  draft: SkinDraft | null;
  onDraft: (draft: SkinDraft | null) => void;
}) {
  const { t } = useTranslation();
  const change = useChangeSkin(accountId);
  const [invalid, setInvalid] = useState(false);
  const heading = useId();

  async function pick(event: ChangeEvent<HTMLInputElement>) {
    const chosen = event.target.files?.[0];
    event.target.value = "";
    if (!chosen) return;
    change.reset();
    const file = await readSkinFile(chosen);
    setInvalid(file === null);
    onDraft(file ? { file, slim: draft?.slim ?? currentSlim } : null);
  }

  function apply() {
    if (!draft) return;
    change.mutate(
      { pngBase64: draft.file.base64, slim: draft.slim },
      { onSuccess: () => onDraft(null) },
    );
  }

  const error = invalid
    ? t("errors.invalid_skin")
    : change.error
      ? t(`errors.${toIpcError(change.error).code}`)
      : null;

  return (
    <section
      aria-labelledby={heading}
      className="flex flex-col gap-4 border-t border-hairline pt-6"
    >
      <h2 id={heading} className="display-md m-0 text-silk">
        {t("accounts.changeSkinTitle")}
      </h2>
      <p className="m-0 max-w-[480px] text-[0.875rem] text-silk-dim">{t("accounts.skinHint")}</p>
      <div className="flex flex-wrap items-center gap-3">
        <label
          className={cx(
            plateLinkClass,
            "cursor-pointer focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-silk",
          )}
        >
          {t("accounts.pickSkin")}
          <input
            type="file"
            accept="image/png"
            className="sr-only"
            disabled={change.isPending}
            onChange={(event) => void pick(event)}
          />
        </label>
      </div>
      {draft ? (
        <div className="flex flex-col gap-4">
          <fieldset className="m-0 flex flex-wrap gap-5 border-0 p-0">
            <legend className="plate mb-2 p-0 text-engrave">{t("accounts.modelLabel")}</legend>
            {(["classic", "slim"] as const).map((model) => (
              <label key={model} className="flex cursor-pointer items-center gap-2 text-silk-dim">
                <input
                  type="radio"
                  name={`${heading}-model`}
                  className="accent-silk"
                  checked={draft.slim === (model === "slim")}
                  onChange={() => onDraft({ ...draft, slim: model === "slim" })}
                />
                {t(`accounts.${model}`)}
              </label>
            ))}
          </fieldset>
          <p className="plate m-0 text-engrave">{t("accounts.previewNote")}</p>
          <div className="flex flex-wrap items-center gap-3">
            <Button onClick={apply} disabled={change.isPending}>
              {change.isPending ? t("accounts.applyingSkin") : t("accounts.applySkin")}
            </Button>
            <Button onClick={() => onDraft(null)} disabled={change.isPending}>
              {t("accounts.discardSkin")}
            </Button>
          </div>
        </div>
      ) : null}
      <p role="status" className="m-0 text-[0.875rem]">
        {error ? (
          <span className="text-stale">{error}</span>
        ) : change.isSuccess ? (
          <span className="text-silk-dim">{t("accounts.skinApplied")}</span>
        ) : null}
      </p>
    </section>
  );
}

/**
 * The capes the account owns, plus "no cape", as toggle buttons: only a click
 * (or Enter) wears one, so moving through them never sends a request.
 */
export function CapePicker({ accountId }: { accountId: string }) {
  const { t } = useTranslation();
  const capes = useAccountCapes(accountId);
  const wear = useSetCape(accountId);
  const heading = useId();

  const list = capes.data ?? [];
  const none = !list.some((cape) => cape.active);
  const options = [
    { id: null, alias: t("accounts.noCape"), active: none, dataUrl: null },
    ...list.map((cape) => ({ ...cape, id: cape.id as string | null })),
  ];

  return (
    <section className="flex flex-col gap-4 border-t border-hairline pt-6">
      <h2 id={heading} className="display-md m-0 text-silk">
        {t("accounts.capesTitle")}
      </h2>
      {capes.isPending ? (
        <p className="m-0 text-engrave">{t("accounts.loadingCapes")}</p>
      ) : capes.isError ? (
        <p className="m-0 text-stale">{t(`errors.${toIpcError(capes.error).code}`)}</p>
      ) : list.length === 0 ? (
        <p className="m-0 text-silk-dim">{t("accounts.noCapes")}</p>
      ) : (
        <fieldset
          aria-labelledby={heading}
          aria-busy={wear.isPending}
          className="m-0 grid min-w-0 border-0 p-0 grid-cols-[repeat(auto-fill,minmax(112px,1fr))] gap-3"
        >
          {options.map((cape) => (
            <button
              key={cape.id ?? "none"}
              type="button"
              aria-pressed={cape.active}
              disabled={wear.isPending}
              onClick={() => {
                if (!cape.active) wear.mutate(cape.id);
              }}
              className={cx(
                "flex cursor-pointer flex-col items-center gap-3 rounded-control border px-3 py-4 text-[0.875rem] transition-colors duration-[160ms] ease-settle disabled:cursor-wait",
                cape.active
                  ? "border-silk-dim bg-panel text-silk"
                  : "border-hairline-lit text-silk-dim hover:border-engrave hover:text-silk",
              )}
            >
              {cape.id ? (
                <CapeSwatch texture={cape.dataUrl} />
              ) : (
                <span
                  aria-hidden="true"
                  className="block h-16 w-10 rounded-plate border border-dashed border-hairline-lit"
                />
              )}
              <span className="truncate">{cape.alias}</span>
            </button>
          ))}
        </fieldset>
      )}
      <p role="status" className="m-0 text-[0.875rem] text-stale">
        {wear.error ? t(`errors.${toIpcError(wear.error).code}`) : null}
      </p>
    </section>
  );
}
