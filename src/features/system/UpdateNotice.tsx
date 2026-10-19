import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { installAppUpdate, type ProgressEvent, toIpcError } from "../../ipc/client";
import { ProgressMeter } from "../instances/ProgressMeter";
import { useAppUpdate } from "./useAppUpdate";

/**
 * A newer launcher is available: say which, show its notes and offer to
 * install it. Installing restarts the launcher, so the backend refuses it
 * while a game installs or runs.
 */
export function UpdateNotice() {
  const { t } = useTranslation();
  const status = useAppUpdate();
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const update = status.data?.kind === "available" ? status.data : null;
  if (!update) return null;

  async function install(version: string) {
    setError(null);
    setInstalling(true);
    try {
      // On success the launcher restarts, so nothing runs after this.
      await installAppUpdate(version, setProgress).done;
    } catch (failure) {
      const code = toIpcError(failure).code;
      if (code === "update_changed") {
        await status.refetch();
      } else {
        setError(code === "already_running" ? t("updates.closeTheGame") : t(`errors.${code}`));
      }
    } finally {
      setInstalling(false);
      setProgress(null);
    }
  }

  return (
    <section
      aria-label={t("updates.title")}
      className="flex flex-col gap-3 border-b border-hairline bg-panel px-6 py-4"
    >
      <div className="flex flex-wrap items-center justify-between gap-4">
        <p className="m-0 text-silk">{t("updates.available", { version: update.version })}</p>
        <Button disabled={installing} onClick={() => void install(update.version)}>
          {t("updates.install")}
        </Button>
      </div>
      {update.notes ? (
        <p className="m-0 whitespace-pre-line text-[0.875rem] text-silk-dim">{update.notes}</p>
      ) : null}
      {progress ? (
        <ProgressMeter
          label={t("updates.downloading")}
          done={progress.done}
          total={progress.total}
          unit={progress.unit}
        />
      ) : null}
      {error ? (
        <p role="alert" className="m-0 text-stale">
          {error}
        </p>
      ) : null}
    </section>
  );
}
