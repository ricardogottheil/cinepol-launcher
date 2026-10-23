import { Link, useNavigate } from "@tanstack/react-router";
import { type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { plateLinkClass } from "../../components/ui/Button";
import { cx } from "../../components/ui/cx";
import { PlayerHead } from "../../components/ui/PlayerHead";
import type { AccountDto } from "../../ipc/client";
import { useAccountSkin, useAccounts, useSetActiveAccount } from "./useAccounts";

const ITEMS = '[role="menuitemradio"], [role="menuitem"]';

const itemClass =
  "flex h-11 w-full cursor-pointer items-center gap-3 px-4 text-left text-silk-dim transition-colors duration-[160ms] ease-settle hover:bg-panel-deep hover:text-silk focus-visible:bg-panel-deep focus-visible:text-silk";

/** The head of a profile: Microsoft profiles show their skin's face. */
function ProfileHead({ account }: { account: AccountDto }) {
  const skin = useAccountSkin(account.kind === "microsoft" ? account.id : null);
  return <PlayerHead skin={skin.data?.dataUrl ?? null} name={account.username} />;
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 12 12"
      className={cx(
        "ml-auto h-3 w-3 shrink-0 text-engrave transition-transform duration-[160ms] ease-settle",
        open && "rotate-180",
      )}
    >
      <path d="M2 7.5 6 3.5l4 4" fill="none" stroke="currentColor" strokeWidth="1.5" />
    </svg>
  );
}

function Check() {
  return (
    <svg aria-hidden="true" viewBox="0 0 12 12" className="ml-auto h-3 w-3 shrink-0 text-silk">
      <path d="m2 6.5 2.5 2.5L10 3.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
    </svg>
  );
}

function ProfileGroup({
  label,
  accounts,
  activeId,
  onPick,
}: {
  label: string;
  accounts: AccountDto[];
  activeId: string | null;
  onPick: (account: AccountDto) => void;
}) {
  if (accounts.length === 0) return null;
  return (
    <fieldset className="m-0 flex min-w-0 flex-col border-0 p-0 py-2">
      <legend className="plate float-left w-full px-4 pt-1 pb-2 text-engrave">{label}</legend>
      {accounts.map((account) => {
        const active = account.id === activeId;
        return (
          <button
            key={account.id}
            type="button"
            role="menuitemradio"
            aria-checked={active}
            tabIndex={-1}
            className={cx(itemClass, active && "text-silk")}
            onClick={() => onPick(account)}
          >
            <ProfileHead account={account} />
            <span className="truncate text-[0.9375rem]">{account.username}</span>
            {active ? <Check /> : null}
          </button>
        );
      })}
    </fieldset>
  );
}

/**
 * The active profile, always at the foot of the sidebar. Opening it lists the
 * online (Microsoft) and offline profiles apart, plus the way to add one.
 */
export function ProfileSwitcher() {
  const { t } = useTranslation();
  const accounts = useAccounts();
  const select = useSetActiveAccount();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const describedBy = useId();

  const list = accounts.data?.accounts ?? [];
  const activeId = accounts.data?.activeId ?? null;
  const active = list.find((account) => account.id === activeId);

  function close(returnFocus: boolean) {
    setOpen(false);
    if (returnFocus) button.current?.focus();
  }

  useEffect(() => {
    if (!open) return;
    const items = menu.current?.querySelectorAll<HTMLElement>(ITEMS);
    const checked = menu.current?.querySelector<HTMLElement>('[aria-checked="true"]');
    (checked ?? items?.[0])?.focus();
    function outside(event: PointerEvent) {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);

  function onMenuKey(event: KeyboardEvent) {
    const items = Array.from(menu.current?.querySelectorAll<HTMLElement>(ITEMS) ?? []);
    const index = items.indexOf(document.activeElement as HTMLElement);
    const move = (to: number) => items[(to + items.length) % items.length]?.focus();
    if (event.key === "Escape") {
      event.preventDefault();
      close(true);
    } else if (event.key === "Tab") {
      close(false);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      move(index + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      move(index - 1);
    } else if (event.key === "Home") {
      event.preventDefault();
      move(0);
    } else if (event.key === "End") {
      event.preventDefault();
      move(items.length - 1);
    }
  }

  function go(to: "/profile" | "/profile/new") {
    close(false);
    void navigate({ to });
  }

  if (!accounts.data) return <div className="h-16 border-t border-hairline" />;

  if (list.length === 0) {
    return (
      <div className="border-t border-hairline px-gutter py-4">
        <Link to="/profile/new" className={cx(plateLinkClass, "w-full justify-center")}>
          {t("accounts.addProfile")}
        </Link>
      </div>
    );
  }

  return (
    <div ref={root} className="relative border-t border-hairline">
      {open ? (
        <div
          ref={menu}
          role="menu"
          aria-label={t("accounts.switcherLabel")}
          onKeyDown={onMenuKey}
          className="absolute right-2 bottom-full left-2 mb-2 flex max-h-[min(420px,calc(100vh-160px))] flex-col overflow-y-auto rounded-control border border-hairline-lit bg-panel"
        >
          <ProfileGroup
            label={t("accounts.onlineGroup")}
            accounts={list.filter((account) => account.kind === "microsoft")}
            activeId={activeId}
            onPick={(account) => {
              select.mutate(account.id);
              close(true);
            }}
          />
          <ProfileGroup
            label={t("accounts.offlineGroup")}
            accounts={list.filter((account) => account.kind === "offline")}
            activeId={activeId}
            onPick={(account) => {
              select.mutate(account.id);
              close(true);
            }}
          />
          <hr className="m-0 border-0 border-t border-hairline" />
          <div className="flex flex-col py-2">
            <button
              type="button"
              role="menuitem"
              tabIndex={-1}
              className={cx(itemClass, "nav-label")}
              onClick={() => go("/profile")}
            >
              {t("accounts.viewProfile")}
            </button>
            <button
              type="button"
              role="menuitem"
              tabIndex={-1}
              className={cx(itemClass, "nav-label")}
              onClick={() => go("/profile/new")}
            >
              {t("accounts.addProfile")}
            </button>
          </div>
        </div>
      ) : null}
      <button
        ref={button}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t("accounts.switcherLabel")}
        aria-describedby={describedBy}
        onClick={() => setOpen((value) => !value)}
        className={cx(
          "flex h-16 w-full cursor-pointer items-center gap-3 px-gutter text-left transition-colors duration-[160ms] ease-settle hover:bg-panel",
          open && "bg-panel",
        )}
      >
        {active ? <ProfileHead account={active} /> : null}
        <span id={describedBy} className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate text-[0.9375rem] text-silk">{active?.username ?? "—"}</span>
          <span className="plate text-engrave">
            {active ? t(`accounts.${active.kind}`) : t("accounts.switcherLabel")}
          </span>
        </span>
        <Chevron open={open} />
      </button>
    </div>
  );
}
