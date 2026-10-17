import { useTranslation } from "react-i18next";

/** Placeholder shell; replaced by the CINEPOL app shell in Milestone 2. */
export function App() {
  const { t } = useTranslation();

  return (
    <div className="flex min-h-screen flex-col bg-surface-base text-ink-primary">
      <main className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center">
        <h1 className="text-4xl font-bold">{t("app.name")}</h1>
        <p className="text-ink-muted">{t("app.scaffoldNotice")}</p>
      </main>
      <footer className="p-4 text-center text-xs text-ink-muted">{t("legal.disclaimer")}</footer>
    </div>
  );
}
