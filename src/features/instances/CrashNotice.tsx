import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { openCrashReport, toIpcError } from "../../ipc/client";

/** The game crashed and wrote a report: say so and offer to open it. */
export function CrashNotice({ instanceId, report }: { instanceId: string; report: string }) {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);

  async function open() {
    setError(null);
    try {
      await openCrashReport(instanceId, report);
    } catch (failure) {
      setError(t(`errors.${toIpcError(failure).code}`));
    }
  }

  return (
    <span className="flex flex-wrap items-center gap-3 text-stale">
      {t("instances.crashed")}
      <Button onClick={() => void open()}>{t("instances.openCrashReport")}</Button>
      {error ? <span>{error}</span> : null}
    </span>
  );
}
