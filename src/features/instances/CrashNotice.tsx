import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { toast } from "../../components/ui/toastStore";
import type { LaunchOutcomeDto } from "../../ipc/client";
import {
  openCrashReport,
  openJvmCrashLog,
  openLogsFolder,
  shareCrashReport,
  toIpcError,
} from "../../ipc/client";

/**
 * The game ended with an error: say so, offer whatever evidence it left, and
 * a link the player can hand to the staff.
 */
export function CrashNotice({ outcome }: { outcome: LaunchOutcomeDto }) {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);
  const [sharing, setSharing] = useState(false);
  const [link, setLink] = useState<string | null>(null);
  const { instanceId, crashReport, jvmCrashLog } = outcome;

  async function run(action: () => Promise<void>) {
    setError(null);
    try {
      await action();
    } catch (failure) {
      setError(t(`errors.${toIpcError(failure).code}`));
    }
  }

  async function share() {
    setSharing(true);
    await run(async () => {
      const shared = await shareCrashReport(instanceId, outcome.exitCode, crashReport, jvmCrashLog);
      setLink(shared.url);
    });
    setSharing(false);
  }

  async function copyLink(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      toast("diagnostics.share.linkCopied", { tone: "success" });
    } catch {
      toast("diagnostics.share.copyFailed");
    }
  }

  return (
    <span className="flex flex-col gap-2">
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
        <Button onClick={() => void run(openLogsFolder)}>{t("chrome.openLogsFolder")}</Button>
        <Button disabled={sharing} onClick={() => void share()}>
          {sharing ? t("instances.sharingCrash") : t("instances.shareCrash")}
        </Button>
      </span>
      {link ? (
        <span className="flex flex-wrap items-center gap-3">
          <a
            href={link}
            target="_blank"
            rel="noreferrer"
            data-readout
            className="readout-sm break-all text-silk"
          >
            {link}
          </a>
          <Button onClick={() => void copyLink(link)}>{t("diagnostics.share.copyLink")}</Button>
        </span>
      ) : null}
      <span className="text-[0.8125rem] text-engrave">{t("instances.shareCrashNote")}</span>
      {error ? <span className="text-stale">{error}</span> : null}
    </span>
  );
}
