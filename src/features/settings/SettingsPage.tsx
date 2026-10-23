import { type FormEvent, useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { TextField } from "../../components/ui/TextField";
import { toIpcError } from "../../ipc/client";
import {
  bundledConfigDto,
  useServerConfig,
  useSetServerConfigUrl,
  useSettings,
} from "../server/useServer";
import { useAppInfo } from "../system/useAppInfo";

export function SettingsPage() {
  const { t } = useTranslation();
  const settings = useSettings();
  const config = useServerConfig().data ?? bundledConfigDto();
  const update = useSetServerConfigUrl();
  const [url, setUrl] = useState("");

  useEffect(() => {
    if (settings.data) setUrl(settings.data.serverConfigUrl);
  }, [settings.data]);

  const error = update.error ? t(`errors.${toIpcError(update.error).code}`) : undefined;

  function submit(event: FormEvent) {
    event.preventDefault();
    update.mutate(url.trim());
  }

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("settings.title")}</h1>
      <form onSubmit={submit} className="flex flex-col gap-5 border-t border-hairline pt-8">
        <h2 className="display-md m-0 text-silk">{t("settings.serverConfigTitle")}</h2>
        <TextField
          label={t("settings.urlLabel")}
          hint={t("settings.serverConfigHint")}
          error={error}
          value={url}
          onChange={(event) => setUrl(event.target.value)}
          spellCheck={false}
          autoComplete="off"
          className="w-full max-w-[640px]"
        />
        <div className="flex flex-wrap items-center gap-3">
          <Button type="submit" disabled={update.isPending || url.trim() === ""}>
            {t("settings.save")}
          </Button>
          <Button
            onClick={() => update.mutate(null)}
            disabled={
              update.isPending ||
              settings.data?.serverConfigUrl === settings.data?.defaultServerConfigUrl
            }
          >
            {t("settings.reset")}
          </Button>
          <span role="status" className="text-[0.875rem] text-engrave">
            {update.isSuccess ? t("settings.saved") : ""}
          </span>
        </div>
        <p className="m-0 text-[0.875rem] text-silk-dim">{t(`settings.origin.${config.origin}`)}</p>
      </form>
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
