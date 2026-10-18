import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  type AccountDto,
  type ClientErrorCode,
  checkPremiumName,
  createOfflineAccount,
  type DeviceCodeDto,
  getAccountPolicy,
  getAccountSkin,
  listAccounts,
  loginMicrosoft,
  type RunningOperation,
  removeAccount,
  setActiveAccount,
  toIpcError,
} from "../../ipc/client";

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

export type MicrosoftSignIn =
  | { kind: "idle"; error: ClientErrorCode | null }
  | { kind: "starting"; cancel: () => Promise<boolean> }
  | { kind: "waiting"; code: DeviceCodeDto; cancel: () => Promise<boolean> };

/** Drives one Microsoft sign-in: the code to show, cancel, and the outcome. */
export function useMicrosoftSignIn() {
  const queryClient = useQueryClient();
  const [state, setState] = useState<MicrosoftSignIn>({ kind: "idle", error: null });

  function start() {
    let operation: RunningOperation<AccountDto> | undefined;
    operation = loginMicrosoft((code) => {
      if (operation) setState({ kind: "waiting", code, cancel: operation.cancel });
    });
    setState({ kind: "starting", cancel: operation.cancel });
    operation.done.then(
      () => {
        setState({ kind: "idle", error: null });
        void queryClient.invalidateQueries({ queryKey: accountsKey });
      },
      (error: unknown) => {
        const code = toIpcError(error).code;
        setState({ kind: "idle", error: code === "cancelled" ? null : code });
      },
    );
  }

  return { state, start };
}
