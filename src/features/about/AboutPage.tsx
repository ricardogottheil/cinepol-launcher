import { Link } from "@tanstack/react-router";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { plateLinkClass } from "../../components/ui/Button";
import { SpecSheet } from "../../components/ui/SpecSheet";
import { useAppInfo } from "../system/useAppInfo";
import { useAppUpdate } from "../system/useAppUpdate";

/**
 * What the launcher is: its version, whether a newer one is out, and the legal
 * notice it must show (AGENTS.md: footer/About). Nothing here is a setting.
 */
export function AboutPage() {
  const { t } = useTranslation();
  const appInfo = useAppInfo();
  const update = useAppUpdate().data;
  const launcher = useId();
  const legal = useId();

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("about.title")}</h1>

      <section
        aria-labelledby={launcher}
        className="flex flex-col gap-6 border-t border-hairline pt-8"
      >
        <h2 id={launcher} className="display-md m-0 text-silk">
          {t("about.launcherTitle")}
        </h2>
        <SpecSheet
          rows={[
            { label: t("chrome.version"), value: appInfo.data?.version ?? "—" },
            {
              label: t("about.updates"),
              value:
                update?.kind === "available"
                  ? t("about.updateAvailable", { version: update.version })
                  : update?.kind === "upToDate"
                    ? t("about.upToDate")
                    : "—",
            },
          ]}
        />
        <Link to="/updates" className={`${plateLinkClass} self-start`}>
          {t("about.openUpdates")}
        </Link>
      </section>

      <section
        aria-labelledby={legal}
        className="flex flex-col gap-4 border-t border-hairline pt-8"
      >
        <h2 id={legal} className="display-md m-0 text-silk">
          {t("about.legalTitle")}
        </h2>
        <p className="m-0 max-w-[640px] text-[0.875rem] leading-[1.45] text-silk-dim">
          {t("legal.disclaimer")}
        </p>
      </section>
    </div>
  );
}
