import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import type { LaunchOutcomeDto } from "../../ipc/client";
import { openCrashReport, openJvmCrashLog, toIpcError } from "../../ipc/client";

/** The game ended with an error: say so and offer whatever evidence it left. */
export function CrashNotice({ outcome }: { outcome: LaunchOutcomeDto }) {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);
  const { instanceId, crashReport, jvmCrashLog } = outcome;

  async function run(open: () => Promise<void>) {
    setError(null);
    try {
      await open();
    } catch (failure) {
      setError(t(`errors.${toIpcError(failure).code}`));
    }
  }

  return (
    <span className="flex flex-wrap items-center gap-3 text-stale">
      {t("instances.crashed", { code: outcome.exitCode ?? "-" })}
      {crashReport ? (
        <Button onClick={() => void run(() => openCrashReport(instanceId, crashReport))}>
          {t("instances.openCrashReport")}
        </Button>
      ) : null}
      {jvmCrashLog ? (
        <Button onClick={() => void run(() => openJvmCrashLog(instanceId, jvmCrashLog))}>
          {t("instances.openJvmCrashLog")}
        </Button>
      ) : null}
      {error ? <span>{error}</span> : null}
    </span>
  );
}
