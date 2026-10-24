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
 * A front view of the player with the arms drawn in full: 4 pixels wide for
 * Classic, 3 for Slim, the one difference between the two models.
 */
function ModelFigure({ slim }: { slim: boolean }) {
  const arm = slim ? 3 : 4;
  return (
    <svg aria-hidden="true" viewBox="0 0 16 32" className="h-12 w-6" shapeRendering="crispEdges">
      <g className="text-hairline-lit" fill="currentColor">
        <rect x="4" y="0" width="8" height="8" />
        <rect x="4" y="8" width="8" height="12" />
        <rect x="4" y="20" width="4" height="12" />
        <rect x="8" y="20" width="4" height="12" />
      </g>
      <g fill="currentColor">
        <rect x={4 - arm} y="8" width={arm} height="12" />
        <rect x="12" y="8" width={arm} height="12" />
      </g>
    </svg>
  );
}

/** Classic or Slim as two plates, chosen before the PNG as on minecraft.net. */
function ModelChoice({
  slim,
  disabled,
  onChange,
}: {
  slim: boolean;
  disabled: boolean;
  onChange: (slim: boolean) => void;
}) {
  const { t } = useTranslation();
  const name = useId();
  return (
    <fieldset className="m-0 flex min-w-0 flex-col gap-3 border-0 p-0">
      <legend className="plate mb-3 p-0 text-engrave">{t("accounts.modelLabel")}</legend>
      <div className="flex flex-wrap gap-3">
        {(["classic", "slim"] as const).map((model) => (
          <label
            key={model}
            className={cx(
              "flex min-w-[132px] cursor-pointer items-center gap-4 rounded-control border border-hairline-lit px-4 py-3 text-silk-dim",
              "transition-colors duration-[160ms] ease-settle hover:border-engrave hover:text-silk",
              "has-checked:border-silk-dim has-checked:bg-panel has-checked:text-silk",
              "focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-silk",
              "has-disabled:cursor-wait",
            )}
          >
            <input
              type="radio"
              name={name}
              className="sr-only"
              checked={slim === (model === "slim")}
              disabled={disabled}
              onChange={() => onChange(model === "slim")}
            />
            <ModelFigure slim={model === "slim"} />
            <span>{t(`accounts.${model}`)}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}

/**
 * Chooses the model, then a PNG, previews both on the 3D figure (the page owns
 * the draft) and applies them with one request. Nothing retries: Mojang may
 * suspend accounts that keep hitting its limit.
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
  // Null until the player picks a model; until then it follows the current skin.
  const [chosenSlim, setChosenSlim] = useState<boolean | null>(null);
  const slim = draft?.slim ?? chosenSlim ?? currentSlim;
  const heading = useId();

  function chooseModel(next: boolean) {
    setChosenSlim(next);
    if (draft) onDraft({ ...draft, slim: next });
  }

  async function pick(event: ChangeEvent<HTMLInputElement>) {
    const chosen = event.target.files?.[0];
    event.target.value = "";
    if (!chosen) return;
    change.reset();
    const file = await readSkinFile(chosen);
    setInvalid(file === null);
    onDraft(file ? { file, slim } : null);
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
      <ModelChoice slim={slim} disabled={change.isPending} onChange={chooseModel} />
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

/** A cape picked on the page but not worn yet; `id` null means no cape. */
export interface CapeChoice {
  id: string | null;
}

/**
 * The capes the account owns, plus "no cape", as toggle buttons. A click only
 * previews one on the figure (the page owns the choice); Apply wears it with
 * one request.
 */
export function CapePicker({
  accountId,
  choice,
  onChoice,
}: {
  accountId: string;
  choice: CapeChoice | null;
  onChoice: (choice: CapeChoice | null) => void;
}) {
  const { t } = useTranslation();
  const capes = useAccountCapes(accountId);
  const wear = useSetCape(accountId);
  const heading = useId();

  const list = capes.data ?? [];
  const worn = list.find((cape) => cape.active)?.id ?? null;
  const selected = choice ? choice.id : worn;
  const options = [
    { id: null, alias: t("accounts.noCape"), dataUrl: null },
    ...list.map((cape) => ({ ...cape, id: cape.id as string | null })),
  ];

  function select(id: string | null) {
    wear.reset();
    onChoice(id === worn ? null : { id });
  }

  function apply() {
    if (!choice) return;
    wear.mutate(choice.id, { onSuccess: () => onChoice(null) });
  }

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
              aria-pressed={cape.id === selected}
              disabled={wear.isPending}
              onClick={() => select(cape.id)}
              className={cx(
                "flex min-w-0 cursor-pointer flex-col items-center gap-3 rounded-control border px-3 py-4 text-[0.875rem] transition-colors duration-[160ms] ease-settle disabled:cursor-wait",
                cape.id === selected
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
              <span className="max-w-full truncate">{cape.alias}</span>
              {cape.id === worn ? (
                <span className="plate -mt-1 text-engrave">{t("accounts.capeWorn")}</span>
              ) : null}
            </button>
          ))}
        </fieldset>
      )}
      {choice ? (
        <div className="flex flex-col gap-4">
          <p className="plate m-0 text-engrave">{t("accounts.previewNote")}</p>
          <div className="flex flex-wrap items-center gap-3">
            <Button onClick={apply} disabled={wear.isPending}>
              {wear.isPending ? t("accounts.applyingCape") : t("accounts.applyCape")}
            </Button>
            <Button onClick={() => onChoice(null)} disabled={wear.isPending}>
              {t("accounts.discardCape")}
            </Button>
          </div>
        </div>
      ) : null}
      <p role="status" className="m-0 text-[0.875rem] text-stale">
        {wear.error ? t(`errors.${toIpcError(wear.error).code}`) : null}
      </p>
    </section>
  );
}
