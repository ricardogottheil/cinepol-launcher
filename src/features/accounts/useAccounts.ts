import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  checkPremiumName,
  createOfflineAccount,
  getAccountPolicy,
  getAccountSkin,
  listAccounts,
  removeAccount,
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

/** The running Microsoft sign-in and how to start one. */
export function useMicrosoftSignIn() {
  const queryClient = useQueryClient();
  const state = useSignInStore((store) => store.signIn);
  const startSignIn = useSignInStore((store) => store.start);
  return {
    state,
    start: () => startSignIn(() => void queryClient.invalidateQueries({ queryKey: accountsKey })),
  };
}
