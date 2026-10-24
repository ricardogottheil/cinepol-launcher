import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  type AccountDto,
  changeAccountSkin,
  checkPremiumName,
  createOfflineAccount,
  getAccountPolicy,
  getAccountSkin,
  listAccountCapes,
  listAccounts,
  removeAccount,
  setAccountCape,
  setActiveAccount,
} from "../../ipc/client";
import { useSignInStore } from "./signInStore";

export const accountsKey = ["accounts"] as const;

export function useAccounts() {
  return useQuery({ queryKey: accountsKey, queryFn: listAccounts });
}

export function useCreateOfflineAccount() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (username: string) => createOfflineAccount(username),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: accountsKey }),
  });
}

export function useSetActiveAccount() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (accountId: string) => setActiveAccount(accountId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: accountsKey }),
  });
}

/** Asks whether a premium account owns a name (`unknown` when the lookup failed). */
export function useCheckPremiumName() {
  return useMutation({ mutationFn: (username: string) => checkPremiumName(username) });
}

export function useAccountPolicy() {
  return useQuery({ queryKey: ["account-policy"], queryFn: getAccountPolicy, staleTime: Infinity });
}

export function useRemoveAccount() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (accountId: string) => removeAccount(accountId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: accountsKey }),
  });
}

/** The skin of a Microsoft account; nothing to fetch for `null`. */
export function useAccountSkin(accountId: string | null) {
  return useQuery({
    queryKey: ["account-skin", accountId],
    queryFn: () => (accountId ? getAccountSkin(accountId) : Promise.resolve(null)),
    enabled: accountId !== null,
    staleTime: 5 * 60_000,
  });
}

/** The capes a Microsoft account owns; nothing to fetch for `null`. */
export function useAccountCapes(accountId: string | null) {
  return useQuery({
    queryKey: ["account-capes", accountId],
    queryFn: () => (accountId ? listAccountCapes(accountId) : Promise.resolve([])),
    enabled: accountId !== null,
    staleTime: 5 * 60_000,
  });
}

/** Uploads a skin. Mutations never retry: Mojang may suspend accounts that keep hitting its limit. */
export function useChangeSkin(accountId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ pngBase64, slim }: { pngBase64: string; slim: boolean }) =>
      changeAccountSkin(accountId, pngBase64, slim ? "slim" : "classic"),
    retry: false,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["account-skin", accountId] }),
  });
}

/**
 * Wears a cape (or none). It settles only after the capes are read again, so
 * "worn" never shows a cape Mojang has not confirmed. A refusal reads them
 * again too: the cape may no longer be the account's.
 */
export function useSetCape(accountId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (capeId: string | null) => setAccountCape(accountId, capeId),
    retry: false,
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["account-capes", accountId] }),
  });
}

/** The running Microsoft sign-in and how to start one; `onAdded` gets the new account. */
export function useMicrosoftSignIn(onAdded?: (account: AccountDto) => void) {
  const queryClient = useQueryClient();
  const state = useSignInStore((store) => store.signIn);
  const startSignIn = useSignInStore((store) => store.start);
  return {
    state,
    start: () =>
      startSignIn((account) => {
        void queryClient.invalidateQueries({ queryKey: accountsKey });
        onAdded?.(account);
      }),
  };
}
