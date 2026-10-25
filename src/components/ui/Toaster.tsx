import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "./cx";
import { IconButton } from "./IconButton";
import { TOAST_MS, type Toast, useToastStore } from "./toastStore";

/** One toast: it leaves after `TOAST_MS`, unless the pointer or focus rests on it. */
function ToastItem({ toast }: { toast: Toast }) {
  const { t } = useTranslation();
  const dismiss = useToastStore((state) => state.dismiss);
  const [held, setHeld] = useState(false);

  useEffect(() => {
    if (held) return;
    const timer = setTimeout(() => dismiss(toast.id), TOAST_MS);
    return () => clearTimeout(timer);
  }, [held, dismiss, toast.id]);

  return (
    <li
      data-toast
      onMouseEnter={() => setHeld(true)}
      onMouseLeave={() => setHeld(false)}
      onFocus={() => setHeld(true)}
      onBlur={() => setHeld(false)}
      className="pointer-events-auto flex items-center gap-3 rounded-control border border-hairline-lit bg-panel py-1 pr-1 pl-4"
    >
      <span
        aria-hidden="true"
        className={cx(
          "size-2 shrink-0 rounded-full",
          toast.tone === "success" ? "bg-signal" : "bg-engrave",
        )}
      />
      <span className="min-w-0 flex-1 break-words text-[0.875rem] leading-[1.4] text-silk">
        {t(toast.message, toast.values ?? {})}
      </span>
      <IconButton label={t("toast.dismiss")} onClick={() => dismiss(toast.id)} className="border-0">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" fill="none" stroke="currentColor" strokeWidth={1} />
        </svg>
      </IconButton>
    </li>
  );
}

/** Where toasts appear: bottom right, over the page, mounted once in the shell. */
export function Toaster() {
  const { t } = useTranslation();
  const toasts = useToastStore((state) => state.toasts);
  return (
    <section
      aria-label={t("toast.region")}
      className="pointer-events-none fixed right-gutter bottom-gutter z-50 w-[min(380px,calc(100vw-2*var(--spacing-gutter)))]"
    >
      {/* The live region is always there, so screen readers hear each toast added to it. */}
      <div role="status">
        <ol className="m-0 flex list-none flex-col gap-2 p-0">
          {toasts.map((toast) => (
            <ToastItem key={toast.id} toast={toast} />
          ))}
        </ol>
      </div>
    </section>
  );
}
