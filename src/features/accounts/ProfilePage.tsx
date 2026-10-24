import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, plateLinkClass } from "../../components/ui/Button";
import { ConfirmDialog } from "../../components/ui/ConfirmDialog";
import { PlayerHead } from "../../components/ui/PlayerHead";
import { SpecSheet } from "../../components/ui/SpecSheet";
import { type AccountDto, toIpcError } from "../../ipc/client";
import { SkinViewer } from "./SkinViewer";
import { type CapeChoice, CapePicker, SkinChanger, type SkinDraft } from "./SkinWardrobe";
import { useAccountCapes, useAccountSkin, useAccounts, useRemoveAccount } from "./useAccounts";

/** Deleting reads in `hazard` and asks first, like deleting an instance. */
function RemoveProfile({ account }: { account: AccountDto }) {
  const { t } = useTranslation();
  const remove = useRemoveAccount();
  const [confirming, setConfirming] = useState(false);
  return (
    <div className="flex flex-wrap items-center gap-3 border-t border-hairline pt-6">
      <Button
        onClick={() => setConfirming(true)}
        disabled={remove.isPending}
        className="border-hazard/60 text-hazard hover:border-hazard hover:text-hazard"
      >
        {t("accounts.remove")}
      </Button>
      <p role="status" className="m-0 text-[0.875rem] text-stale">
        {remove.error ? t(`errors.${toIpcError(remove.error).code}`) : null}
      </p>
      {confirming ? (
        <ConfirmDialog
          destructive
          title={t("accounts.removeTitle", { name: account.username })}
          confirmLabel={t("accounts.confirmRemove")}
          cancelLabel={t("accounts.keep")}
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            remove.mutate(account.id);
          }}
        >
          <p className="m-0">{t("accounts.removeBody")}</p>
        </ConfirmDialog>
      ) : null}
    </div>
  );
}

function Profile({ account }: { account: AccountDto }) {
  const { t } = useTranslation();
  const premium = account.kind === "microsoft";
  const skin = useAccountSkin(premium ? account.id : null);
  const capes = useAccountCapes(premium ? account.id : null);
  // A picked model, skin or cape shows on the figure before it is applied.
  const [draft, setDraft] = useState<SkinDraft | null>(null);
  // Null until the player picks a model; until then it follows the current skin.
  const [chosenSlim, setChosenSlim] = useState<boolean | null>(null);
  const slim = draft?.slim ?? chosenSlim ?? skin.data?.slim ?? false;
  const [pickedCape, setCapeChoice] = useState<CapeChoice | null>(null);
  // A picked cape the account no longer owns is no choice at all.
  const capeChoice =
    pickedCape?.id === null || capes.data?.some((owned) => owned.id === pickedCape?.id)
      ? pickedCape
      : null;
  const shownCape = capeChoice
    ? capes.data?.find((owned) => owned.id === capeChoice.id)
    : capes.data?.find((owned) => owned.active);
  const cape = shownCape?.dataUrl ?? null;
  return (
    <div className="grid grid-cols-1 gap-10 @3xl:grid-cols-[320px_1fr]">
      <section aria-label={t("accounts.skinTitle")} className="flex flex-col gap-4">
        {premium ? (
          <SkinViewer
            texture={draft?.file.dataUrl ?? skin.data?.dataUrl ?? null}
            slim={slim}
            cape={cape}
            label={t("accounts.skinLabel")}
            className="w-full"
          />
        ) : (
          <p className="m-0 border-t border-hairline pt-6 text-silk-dim">
            {t("accounts.skinOffline")}
          </p>
        )}
      </section>
      <div className="flex flex-col gap-8">
        <SpecSheet
          rows={[
            { label: t("accounts.usernameLabel"), value: account.username },
            { label: t("accounts.kindLabel"), value: t(`accounts.${account.kind}`) },
          ]}
        />
        {premium ? (
          <>
            <SkinChanger
              accountId={account.id}
              slim={slim}
              onModel={setChosenSlim}
              draft={draft}
              onDraft={setDraft}
            />
            <CapePicker accountId={account.id} choice={capeChoice} onChoice={setCapeChoice} />
          </>
        ) : null}
        <RemoveProfile account={account} />
      </div>
    </div>
  );
}

/** The active profile: who plays, how they look in game, and how to remove it. */
export function ProfilePage() {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const active = accounts.data?.accounts.find((account) => account.id === accounts.data?.activeId);
  const skin = useAccountSkin(active?.kind === "microsoft" ? active.id : null);

  if (accounts.isError) {
    return (
      <div className="mx-auto max-w-[1120px] px-gutter-lg py-10">
        <p className="m-0 text-stale">{t(`errors.${toIpcError(accounts.error).code}`)}</p>
      </div>
    );
  }
  if (!accounts.data) return null;

  return (
    <div className="@container mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      {active ? (
        <>
          <header className="flex items-center gap-5">
            <PlayerHead skin={skin.data?.dataUrl ?? null} name={active.username} size="lg" />
            <h1 className="display-lg m-0 min-w-0 break-words text-silk">{active.username}</h1>
          </header>
          {/* Keyed so a pending removal never carries over to another profile. */}
          <Profile key={active.id} account={active} />
        </>
      ) : (
        <div className="flex flex-col items-start gap-5">
          <h1 className="display-lg m-0 text-silk">{t("nav.profile")}</h1>
          <p className="m-0 text-silk-dim">{t("accounts.noProfile")}</p>
          <Link to="/profile/new" className={plateLinkClass}>
            {t("accounts.addProfile")}
          </Link>
        </div>
      )}
    </div>
  );
}
