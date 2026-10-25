import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import { useAppUpdate } from "./useAppUpdate";

/**
 * In the sidebar, above the profile: a newer launcher is available. Opens the
 * update page; nothing shows while the launcher is up to date.
 */
export function UpdateBadge() {
  const { t } = useTranslation();
  const status = useAppUpdate();
  const update = status.data?.kind === "available" ? status.data : null;
  if (!update) return null;
  return (
    <Link
      to="/updates"
      className="relative flex flex-col gap-1 border-t border-hairline px-gutter py-3 no-underline transition-colors duration-[160ms] ease-settle before:absolute before:inset-y-3 before:left-0 before:w-[3px] before:bg-signal hover:bg-panel"
      activeProps={{ className: "bg-panel", "aria-current": "page" }}
    >
      <span className="plate text-signal">{t("updates.badge", { version: update.version })}</span>
      <span className="text-[0.875rem] text-silk-dim">{t("updates.seeWhatsNew")}</span>
    </Link>
  );
}
