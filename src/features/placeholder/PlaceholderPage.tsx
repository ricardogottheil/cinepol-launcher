import { useTranslation } from "react-i18next";

export type PlaceholderSection = "modpacks" | "settings";

/** A section whose feature lands in a later milestone. */
export function PlaceholderPage({ section }: { section: PlaceholderSection }) {
  const { t } = useTranslation();
  return (
    <div className="mx-auto max-w-[1120px] px-gutter-lg py-12">
      <h1 className="display-lg m-0 text-silk">{t(`nav.${section}`)}</h1>
      <p className="mt-6 mb-0 border-t border-hairline pt-6 text-silk-dim">
        {t("placeholder.notAvailable")}
      </p>
    </div>
  );
}
