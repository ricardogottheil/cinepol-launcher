import { create } from "zustand";

/** How long a toast stays when nobody points at it. */
export const TOAST_MS = 5000;

/** At most this many toasts show at once; older ones make room. */
export const MAX_TOASTS = 3;

/** `success` confirms something the player did; `info` reports what happened instead. */
export type ToastTone = "success" | "info";

export interface Toast {
  id: number;
  tone: ToastTone;
  /** An i18n key, translated when it shows so a language change applies. */
  message: string;
  values?: Record<string, string | number>;
}

interface ToastState {
  toasts: Toast[];
  dismiss: (id: number) => void;
}

let nextId = 1;

/** Short-lived notices of what just happened, shown by the `Toaster`. */
export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  dismiss: (id) => set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) })),
}));

/** Shows a toast; usable outside React (stores, mutation callbacks). */
export function toast(
  message: string,
  { tone = "info", values }: { tone?: ToastTone; values?: Toast["values"] } = {},
): number {
  const id = nextId++;
  const next: Toast = values ? { id, tone, message, values } : { id, tone, message };
  useToastStore.setState((state) => ({ toasts: [...state.toasts, next].slice(-MAX_TOASTS) }));
  return id;
}
