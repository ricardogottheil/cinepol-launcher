import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { TextField } from "../../components/ui/TextField";
import { useModpackJobStore } from "./modpackJobStore";

/** Packs from a file (Modrinth .mrpack, CurseForge .zip) or from packwiz. */
export function ImportTab({ hasCurseForge }: { hasCurseForge: boolean }) {
  const { t } = useTranslation();
  const { job, importFile, importCurseForgeFile, installPackwizUrl, importPackwizFile } =
    useModpackJobStore();
  const [packUrl, setPackUrl] = useState("");
  const busy = job.kind !== "idle";
  const files = useId();
  const packwiz = useId();

  return (
    <div className="flex flex-col gap-10">
      <section aria-labelledby={files} className="flex flex-col gap-4">
        <h2 id={files} className="display-md m-0 text-silk">
          {t("modpacks.importFiles")}
        </h2>
        <p className="m-0 max-w-[560px] text-silk-dim">{t("modpacks.importFilesHint")}</p>
        <div className="flex flex-wrap gap-3">
          <Button onClick={() => void importFile()} disabled={busy}>
            {t("modpacks.import")}
          </Button>
          {hasCurseForge ? (
            <Button onClick={() => void importCurseForgeFile()} disabled={busy}>
              {t("modpacks.importZip")}
            </Button>
          ) : null}
        </div>
      </section>

      <section
        aria-labelledby={packwiz}
        className="flex flex-col gap-4 border-t border-hairline pt-8"
      >
        <h2 id={packwiz} className="display-md m-0 text-silk">
          {t("modpacks.packwiz.title")}
        </h2>
        <span className="plate text-engrave">{t("modpacks.packwiz.source")}</span>
        <form
          className="flex flex-wrap items-end gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            void installPackwizUrl(packUrl.trim());
          }}
        >
          <TextField
            label={t("modpacks.packwiz.urlLabel")}
            hint={t("modpacks.packwiz.urlHint")}
            type="url"
            inputMode="url"
            placeholder="https://"
            value={packUrl}
            className="min-w-[320px] flex-1"
            onChange={(event) => setPackUrl(event.target.value)}
          />
          <Button type="submit" disabled={busy || packUrl.trim() === ""}>
            {t("modpacks.packwiz.install")}
          </Button>
          <Button onClick={() => void importPackwizFile()} disabled={busy}>
            {t("modpacks.packwiz.import")}
          </Button>
        </form>
      </section>
    </div>
  );
}
