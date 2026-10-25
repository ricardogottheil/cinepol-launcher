import { useId } from "react";
import { useTranslation } from "react-i18next";
import { SignalControl } from "../../components/ui/SignalControl";
import { SpecSheet } from "../../components/ui/SpecSheet";
import { toIpcError } from "../../ipc/client";
import { ProgressMeter } from "../instances/ProgressMeter";
import { useAppUpdateInstall } from "./appUpdateStore";
import { parseReleaseNotes } from "./releaseNotes";
import { useAppInfo } from "./useAppInfo";
import { useAppUpdate } from "./useAppUpdate";

/** The release notes of the offered version, in the player's language. */
function ReleaseNotes({ notes }: { notes: string }) {
  const { t, i18n } = useTranslation();
  const heading = useId();
  const blocks = parseReleaseNotes(notes, i18n.language);
  if (blocks.length === 0) return null;
  return (
    <section
      aria-labelledby={heading}
      className="flex flex-col gap-4 border-t border-hairline pt-8"
    >
      <h2 id={heading} className="display-md m-0 text-silk">
        {t("updates.whatsNew")}
      </h2>
      {blocks.map((block, index) =>
        block.kind === "paragraph" ? (
          // Notes never change while shown, so the position is a stable key.
          // biome-ignore lint/suspicious/noArrayIndexKey: static content
          <p key={index} className="m-0 max-w-[720px] text-silk-dim">
            {block.text}
          </p>
        ) : (
          // biome-ignore lint/suspicious/noArrayIndexKey: static content
          <ul key={index} className="m-0 flex max-w-[720px] list-none flex-col p-0">
            {block.items.map((item, position) => (
              <li
                // Notes may repeat a line, so the text alone is not a key.
                // biome-ignore lint/suspicious/noArrayIndexKey: static content
                key={position}
                className="relative border-b border-hairline py-3 pl-5 text-silk before:absolute before:top-[1.35em] before:left-0 before:size-1.5 before:bg-signal"
              >
                {item}
              </li>
            ))}
          </ul>
        ),
      )}
    </section>
  );
}

/**
 * The launcher's own update: which version is offered, what it brings, and
 * the control that installs it. Installing restarts the launcher, so the
 * backend refuses it while a game installs or runs.
 */
export function UpdatePage() {
  const { t } = useTranslation();
  const status = useAppUpdate();
  const appInfo = useAppInfo();
  const { installing, progress, error: code, install: start } = useAppUpdateInstall();
  const error = code
    ? code === "already_running"
      ? t("updates.closeTheGame")
      : t(`errors.${code}`)
    : null;

  async function install(version: string) {
    if (await start(version)) await status.refetch();
  }

  const update = status.data?.kind === "available" ? status.data : null;

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("updates.title")}</h1>

      {update ? (
        <>
          <div className="flex flex-col gap-8">
            <SpecSheet
              rows={[
                { label: t("updates.currentVersion"), value: update.currentVersion },
                { label: t("updates.newVersion"), value: update.version },
              ]}
            />
            {installing ? (
              <div className="flex max-w-[560px] flex-col gap-3 rounded-control border border-hairline-lit p-4">
                <span className="plate text-silk">{t("updates.installing")}</span>
                <ProgressMeter
                  label={t("updates.downloading")}
                  done={progress?.done ?? 0}
                  total={progress?.total ?? null}
                  unit={progress?.unit ?? "bytes"}
                />
              </div>
            ) : (
              <SignalControl
                label={t("updates.install")}
                detail={t("updates.product", { version: update.version })}
                className="max-w-[560px]"
                onPress={() => void install(update.version)}
              />
            )}
            {error ? (
              <p role="alert" className="m-0 text-stale">
                {error}
              </p>
            ) : null}
          </div>
          {update.notes ? <ReleaseNotes notes={update.notes} /> : null}
        </>
      ) : (
        <div className="flex flex-col gap-2 border-t border-hairline pt-8">
          <p role="status" className="display-md m-0 text-silk">
            {status.isPending
              ? t("updates.checking")
              : status.isError
                ? t(`errors.${toIpcError(status.error).code}`)
                : status.data?.kind === "notConfigured"
                  ? t("updates.notConfigured")
                  : t("updates.upToDate")}
          </p>
          {appInfo.data && status.data?.kind === "upToDate" ? (
            <p className="m-0 text-silk-dim">
              {t("updates.upToDateDetail", { version: appInfo.data.version })}
            </p>
          ) : null}
        </div>
      )}
    </div>
  );
}
