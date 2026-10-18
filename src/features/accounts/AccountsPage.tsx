import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { TextField } from "../../components/ui/TextField";
import { type AccountDto, openExternal, toIpcError } from "../../ipc/client";
import { SkinViewer } from "./SkinViewer";
import {
  useAccountPolicy,
  useAccountSkin,
  useAccounts,
  useCheckPremiumName,
  useCreateOfflineAccount,
  useMicrosoftSignIn,
  useRemoveAccount,
  useSetActiveAccount,
} from "./useAccounts";

function AccountRow({ account, active }: { account: AccountDto; active: boolean }) {
  const { t } = useTranslation();
  const select = useSetActiveAccount();
  const remove = useRemoveAccount();
  const [confirming, setConfirming] = useState(false);
  return (
    <li className="flex flex-wrap items-center justify-between gap-6 border-b border-hairline py-5">
      <div className="flex flex-col gap-1">
        <span data-readout className="readout-md text-silk">
          {account.username}
        </span>
        <span className="plate text-engrave">{t(`accounts.${account.kind}`)}</span>
      </div>
      <div className="flex flex-wrap items-center gap-3">
        {active ? (
          <span className="plate text-silk">{t("accounts.active")}</span>
        ) : (
          <Button onClick={() => select.mutate(account.id)} disabled={select.isPending}>
            {t("accounts.makeActive")}
          </Button>
        )}
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
      </div>
    </li>
  );
}

/** The device-code sign-in: a button, then the code to type and where. */
function MicrosoftSignInPanel() {
  const { t } = useTranslation();
  const policy = useAccountPolicy();
  const { state, start } = useMicrosoftSignIn();

  if (!policy.data) return null;
  if (!policy.data.microsoftAvailable) {
    return (
      <section className="flex flex-col gap-3 border-t border-hairline pt-8">
        <h2 className="display-md m-0 text-silk">{t("accounts.microsoftTitle")}</h2>
        <p className="m-0 text-silk-dim">{t("accounts.microsoftUnavailable")}</p>
      </section>
    );
  }

  return (
    <section className="flex flex-col gap-5 border-t border-hairline pt-8">
      <h2 className="display-md m-0 text-silk">{t("accounts.microsoftTitle")}</h2>
      {state.kind === "idle" ? (
        <>
          <p className="m-0 text-silk-dim">{t("accounts.microsoftHint")}</p>
          <Button className="self-start" onClick={start}>
            {t("accounts.microsoftSignIn")}
          </Button>
        </>
      ) : (
        <div className="flex max-w-[640px] flex-col gap-4 rounded-control border border-hairline-lit p-5">
          {state.kind === "waiting" ? (
            <>
              <span className="plate text-engrave">{t("accounts.codeLabel")}</span>
              <span data-readout className="readout-xl select-all text-silk">
                {state.code.userCode}
              </span>
              <p className="m-0 text-silk-dim">
                {t("accounts.codeInstructions", { uri: state.code.verificationUri })}
              </p>
              <div className="flex flex-wrap gap-3">
                <Button onClick={() => void openExternal(state.code.verificationUri)}>
                  {t("accounts.openLink")}
                </Button>
                <Button onClick={() => void navigator.clipboard?.writeText(state.code.userCode)}>
                  {t("accounts.copyCode")}
                </Button>
                <Button onClick={() => void state.cancel()}>{t("accounts.cancelSignIn")}</Button>
              </div>
            </>
          ) : (
            <div className="flex items-center justify-between gap-4">
              <span className="plate text-silk">{t("accounts.waiting")}</span>
              <Button onClick={() => void state.cancel()}>{t("accounts.cancelSignIn")}</Button>
            </div>
          )}
        </div>
      )}
      <p role="status" className="m-0 max-w-[640px] text-[0.875rem] text-stale">
        {state.kind === "idle" && state.error ? t(`errors.${state.error}`) : null}
      </p>
    </section>
  );
}

function OfflineAccountForm({ blocked }: { blocked: boolean }) {
  const { t } = useTranslation();
  const [username, setUsername] = useState("");
  const create = useCreateOfflineAccount();
  const checkName = useCheckPremiumName();
  const error = create.error ? t(`errors.${toIpcError(create.error).code}`) : undefined;

  function submit(event: FormEvent) {
    event.preventDefault();
    const name = username.trim();
    checkName.reset();
    create.mutate(name, {
      onSuccess: () => {
        setUsername("");
        // Never blocks: the profile exists already; a failed lookup warns nothing.
        checkName.mutate(name);
      },
    });
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-5 border-t border-hairline pt-8">
      <h2 className="display-md m-0 text-silk">{t("accounts.offlineTitle")}</h2>
      {blocked ? (
        <p className="m-0 max-w-[640px] text-silk-dim">{t("accounts.offlineRequiresPremium")}</p>
      ) : null}
      <div className="flex flex-wrap items-start gap-4">
        <TextField
          label={t("accounts.usernameLabel")}
          hint={t("accounts.offlineHint")}
          error={error}
          value={username}
          onChange={(event) => setUsername(event.target.value)}
          maxLength={16}
          autoComplete="off"
          spellCheck={false}
          disabled={blocked}
          className="w-full max-w-[420px]"
        />
        <Button
          type="submit"
          className="mt-[26px]"
          disabled={blocked || create.isPending || username.trim() === ""}
        >
          {t("accounts.create")}
        </Button>
      </div>
      <p role="status" className="m-0 max-w-[640px] text-[0.875rem] text-stale">
        {checkName.data === "premium" ? t("accounts.premiumNameWarning") : null}
      </p>
    </form>
  );
}

/** The active account's skin in 3D; offline accounts have none to show. */
function ActiveSkin({ account }: { account: AccountDto | undefined }) {
  const { t } = useTranslation();
  const premiumId = account?.kind === "microsoft" ? account.id : null;
  const skin = useAccountSkin(premiumId);
  if (!account) return null;
  return (
    <section className="flex flex-col gap-4">
      <h2 className="plate m-0 text-engrave">{t("accounts.skinTitle")}</h2>
      {premiumId ? (
        <SkinViewer
          texture={skin.data?.dataUrl ?? null}
          slim={skin.data?.slim ?? false}
          label={t("accounts.skinLabel")}
          className="w-full max-w-[280px]"
        />
      ) : (
        <p className="m-0 max-w-[280px] text-[0.875rem] text-silk-dim">
          {t("accounts.skinOffline")}
        </p>
      )}
    </section>
  );
}

export function AccountsPage() {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const policy = useAccountPolicy();
  const list = accounts.data?.accounts ?? [];
  const active = list.find((account) => account.id === accounts.data?.activeId);
  const offlineBlocked =
    (policy.data?.requirePremiumForOffline ?? false) &&
    !list.some((account) => account.kind === "microsoft");

  return (
    <div className="@container mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("accounts.title")}</h1>
      <div className="grid grid-cols-1 gap-10 @3xl:grid-cols-[1fr_280px]">
        <section aria-label={t("accounts.title")}>
          {accounts.isPending ? (
            <p className="m-0 text-engrave">{t("accounts.loading")}</p>
          ) : accounts.isError ? (
            <p className="m-0 text-stale">{t(`errors.${toIpcError(accounts.error).code}`)}</p>
          ) : list.length === 0 ? (
            <p className="m-0 border-t border-hairline pt-6 text-silk-dim">{t("accounts.empty")}</p>
          ) : (
            <ul className="m-0 list-none border-t border-hairline p-0">
              {list.map((account) => (
                <AccountRow
                  key={account.id}
                  account={account}
                  active={account.id === accounts.data?.activeId}
                />
              ))}
            </ul>
          )}
        </section>
        <ActiveSkin account={active} />
      </div>
      <MicrosoftSignInPanel />
      <OfflineAccountForm blocked={offlineBlocked} />
    </div>
  );
}
