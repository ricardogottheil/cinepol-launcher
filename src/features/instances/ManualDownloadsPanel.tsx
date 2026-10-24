import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { type ManualDownloadsDto, openExternal, toIpcError } from "../../ipc/client";

/**
 * Files the player downloads by hand (CurseForge files the launcher may not
 * fetch). The launcher keeps checking the Downloads folder and installs each
 * file once its hash matches.
 */
export function ManualDownloadsPanel({
  downloads,
  error,
  checking,
  onCheck,
}: {
  downloads: ManualDownloadsDto;
  error: unknown;
  checking: boolean;
  onCheck: () => void;
}) {
  const { t } = useTranslation();
  const remaining = downloads.remaining;

  return (
    <section
      aria-label={t("instances.manual.title")}
      className="flex flex-col gap-4 rounded-control border border-hazard bg-hazard-ground p-5"
    >
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h3 className="plate m-0 text-hazard">{t("instances.manual.title")}</h3>
        <Button onClick={onCheck} disabled={checking}>
          {t("instances.manual.check")}
        </Button>
      </div>
      {remaining.length === 0 ? (
        <p role="status" className="m-0 text-silk">
          {t("instances.manual.done")}
        </p>
      ) : (
        <>
          <p className="m-0 text-silk-dim">{t("instances.manual.body")}</p>
          <ul className="m-0 list-none divide-y divide-hairline p-0">
            {remaining.map((file) => (
              <li
                key={file.filename}
                className="flex flex-wrap items-center justify-between gap-4 py-3"
              >
                <div className="flex min-w-0 flex-col gap-1">
                  <span className="text-silk">{file.name}</span>
                  <span className="readout-sm break-all text-engrave">{file.filename}</span>
                </div>
                <Button onClick={() => void openExternal(file.pageUrl)}>
                  {t("instances.manual.open")}
                </Button>
              </li>
            ))}
          </ul>
          <p role="status" className="m-0 text-[0.875rem] text-engrave">
            {error
              ? t(`errors.${toIpcError(error).code}`)
              : t("instances.manual.waiting", { count: remaining.length })}
          </p>
        </>
      )}
    </section>
  );
}
