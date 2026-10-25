import { useId } from "react";
import { useTranslation } from "react-i18next";
import { useAppInfo } from "../system/useAppInfo";

export function SettingsPage() {
  const { t } = useTranslation();
  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("settings.title")}</h1>
      <About />
    </div>
  );
}

/** The version and the legal notice the launcher must show (AGENTS.md: footer/About). */
function About() {
  const { t } = useTranslation();
  const appInfo = useAppInfo();
  const heading = useId();
  return (
    <section
      aria-labelledby={heading}
      className="flex flex-col gap-4 border-t border-hairline pt-8"
    >
      <h2 id={heading} className="display-md m-0 text-silk">
        {t("settings.aboutTitle")}
      </h2>
      {appInfo.data ? (
        <p className="plate m-0 flex items-baseline gap-2 text-engrave">
          {t("chrome.version")}
          <span data-readout className="readout-sm normal-case tracking-normal text-silk">
            {appInfo.data.version}
          </span>
        </p>
      ) : null}
      <p className="m-0 max-w-[640px] text-[0.875rem] leading-[1.45] text-silk-dim">
        {t("legal.disclaimer")}
      </p>
    </section>
  );
}
