import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { TextField } from "../../components/ui/TextField";
import { type AccountDto, toIpcError } from "../../ipc/client";
import { useAccounts, useCreateOfflineAccount, useSetActiveAccount } from "./useAccounts";

function AccountRow({ account, active }: { account: AccountDto; active: boolean }) {
  const { t } = useTranslation();
  const select = useSetActiveAccount();
  return (
    <li className="flex items-center justify-between gap-6 border-b border-hairline py-5">
      <div className="flex flex-col gap-1">
        <span data-readout className="readout-md text-silk">
          {account.username}
        </span>
        <span className="plate text-engrave">{t(`accounts.${account.kind}`)}</span>
      </div>
      {active ? (
        <span className="plate text-silk">{t("accounts.active")}</span>
      ) : (
        <Button onClick={() => select.mutate(account.id)} disabled={select.isPending}>
          {t("accounts.makeActive")}
        </Button>
      )}
    </li>
  );
}

function OfflineAccountForm() {
  const { t } = useTranslation();
  const [username, setUsername] = useState("");
  const create = useCreateOfflineAccount();
  const error = create.error ? t(`errors.${toIpcError(create.error).code}`) : undefined;

  function submit(event: FormEvent) {
    event.preventDefault();
    create.mutate(username.trim(), { onSuccess: () => setUsername("") });
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-5 border-t border-hairline pt-8">
      <h2 className="display-md m-0 text-silk">{t("accounts.offlineTitle")}</h2>
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
          className="w-full max-w-[420px]"
        />
        <Button
          type="submit"
          className="mt-[26px]"
          disabled={create.isPending || username.trim() === ""}
        >
          {t("accounts.create")}
        </Button>
      </div>
    </form>
  );
}

export function AccountsPage() {
  const { t } = useTranslation();
  const accounts = useAccounts();

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("accounts.title")}</h1>
      <section aria-label={t("accounts.title")}>
        {accounts.isPending ? (
          <p className="m-0 text-engrave">{t("accounts.loading")}</p>
        ) : accounts.isError ? (
          <p className="m-0 text-stale">{t(`errors.${toIpcError(accounts.error).code}`)}</p>
        ) : accounts.data.accounts.length === 0 ? (
          <p className="m-0 border-t border-hairline pt-6 text-silk-dim">{t("accounts.empty")}</p>
        ) : (
          <ul className="m-0 list-none border-t border-hairline p-0">
            {accounts.data.accounts.map((account) => (
              <AccountRow
                key={account.id}
                account={account}
                active={account.id === accounts.data.activeId}
              />
            ))}
          </ul>
        )}
      </section>
      <OfflineAccountForm />
    </div>
  );
}
