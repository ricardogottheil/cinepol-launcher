import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { TextField } from "../../components/ui/TextField";
import { type AccountDto, openExternal, toIpcError } from "../../ipc/client";
import {
  useAccountPolicy,
  useAccounts,
  useCheckPremiumName,
  useCreateOfflineAccount,
  useMicrosoftSignIn,
  useSetActiveAccount,
} from "./useAccounts";

/** The device-code sign-in: a button, then the code to type and where. */
function MicrosoftSignInPanel({ onAdded }: { onAdded: (account: AccountDto) => void }) {
  const { t } = useTranslation();
  const policy = useAccountPolicy();
  const { state, start } = useMicrosoftSignIn(onAdded);

  if (!policy.data) return null;
  if (!policy.data.microsoftAvailable) {
    return (
      <section className="flex flex-col gap-3 border-t border-hairline pt-6">
        <h2 className="display-md m-0 text-silk">{t("accounts.microsoftTitle")}</h2>
        <p className="m-0 text-silk-dim">{t("accounts.microsoftUnavailable")}</p>
      </section>
    );
  }

  return (
    <section className="flex flex-col gap-5 border-t border-hairline pt-6">
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

function OfflineAccountForm({
  blocked,
  onAdded,
}: {
  blocked: boolean;
  onAdded: (account: AccountDto) => void;
}) {
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
      onSuccess: (account) => {
        setUsername("");
        onAdded(account);
        // Never blocks: the profile exists already; a failed lookup warns nothing.
        checkName.mutate(name);
      },
    });
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-5 border-t border-hairline pt-6">
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

/** Adds a profile: a Microsoft account (online) or an offline name. */
export function AddProfilePage() {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const policy = useAccountPolicy();
  const select = useSetActiveAccount();
  const [added, setAdded] = useState<AccountDto | null>(null);
  const list = accounts.data?.accounts ?? [];
  const offlineBlocked =
    (policy.data?.requirePremiumForOffline ?? false) &&
    !list.some((account) => account.kind === "microsoft");
  const addedIsActive = added !== null && added.id === accounts.data?.activeId;

  return (
    <div className="@container mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("accounts.addTitle")}</h1>
      <div role="status" className="empty:hidden">
        {added ? (
          <div className="flex flex-wrap items-center gap-4 rounded-control border border-hairline-lit p-4">
            <span className="text-silk">{t("accounts.added", { name: added.username })}</span>
            {addedIsActive ? null : (
              <Button onClick={() => select.mutate(added.id)} disabled={select.isPending}>
                {t("accounts.useProfile")}
              </Button>
            )}
          </div>
        ) : null}
      </div>
      <div className="grid grid-cols-1 gap-10 @3xl:grid-cols-2">
        <MicrosoftSignInPanel onAdded={setAdded} />
        <OfflineAccountForm blocked={offlineBlocked} onAdded={setAdded} />
      </div>
    </div>
  );
}
