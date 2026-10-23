import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, plateLinkClass } from "../../components/ui/Button";
import { PlayerHead } from "../../components/ui/PlayerHead";
import { SpecSheet } from "../../components/ui/SpecSheet";
import { type AccountDto, toIpcError } from "../../ipc/client";
import { SkinViewer } from "./SkinViewer";
import { useAccountSkin, useAccounts, useRemoveAccount } from "./useAccounts";

function RemoveProfile({ account }: { account: AccountDto }) {
  const { t } = useTranslation();
  const remove = useRemoveAccount();
  const [confirming, setConfirming] = useState(false);
  return (
    <div className="flex flex-wrap items-center gap-3 border-t border-hairline pt-6">
      {confirming ? (
        <>
          <Button onClick={() => remove.mutate(account.id)} disabled={remove.isPending}>
            {t("accounts.confirmRemove")}
          </Button>
          <Button onClick={() => setConfirming(false)}>{t("accounts.keep")}</Button>
        </>
      ) : (
        <Button onClick={() => setConfirming(true)}>{t("accounts.remove")}</Button>
      )}
      <p role="status" className="m-0 text-[0.875rem] text-stale">
        {remove.error ? t(`errors.${toIpcError(remove.error).code}`) : null}
      </p>
    </div>
  );
}

function Profile({ account }: { account: AccountDto }) {
  const { t } = useTranslation();
  const premium = account.kind === "microsoft";
  const skin = useAccountSkin(premium ? account.id : null);
  return (
    <div className="grid grid-cols-1 gap-10 @3xl:grid-cols-[320px_1fr]">
      <section aria-label={t("accounts.skinTitle")} className="flex flex-col gap-4">
        {premium ? (
          <SkinViewer
            texture={skin.data?.dataUrl ?? null}
            slim={skin.data?.slim ?? false}
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
          <Profile account={active} />
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
