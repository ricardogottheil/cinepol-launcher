import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { SelectField } from "../../components/ui/SelectField";
import { TextField } from "../../components/ui/TextField";
import { toIpcError } from "../../ipc/client";
import { useCreateInstance, useGameVersions } from "./useInstances";

export function NewInstanceForm() {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [snapshots, setSnapshots] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);
  const versions = useGameVersions(snapshots);
  const create = useCreateInstance();

  const options = (versions.data?.versions ?? []).map((version) => ({
    value: version.id,
    label: version.id,
  }));
  const gameVersion = picked ?? versions.data?.latestRelease ?? "";
  const error = create.error ? t(`errors.${toIpcError(create.error).code}`) : undefined;

  function submit(event: FormEvent) {
    event.preventDefault();
    create.mutate({ name: name.trim(), gameVersion }, { onSuccess: () => setName("") });
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-5 border-t border-hairline pt-8">
      <h2 className="display-md m-0 text-silk">{t("instances.newTitle")}</h2>
      <div className="flex flex-wrap items-start gap-4">
        <TextField
          label={t("instances.nameLabel")}
          value={name}
          error={error}
          onChange={(event) => setName(event.target.value)}
          maxLength={64}
          className="w-full max-w-[360px]"
        />
        <SelectField
          label={t("instances.versionLabel")}
          options={options}
          value={gameVersion}
          onChange={(event) => setPicked(event.target.value)}
          disabled={options.length === 0}
          className="w-[200px]"
        />
        <Button
          type="submit"
          className="mt-[26px]"
          disabled={create.isPending || name.trim() === "" || gameVersion === ""}
        >
          {t("instances.create")}
        </Button>
      </div>
      <label className="flex items-center gap-3 text-[0.875rem] text-silk-dim">
        <input
          type="checkbox"
          checked={snapshots}
          onChange={(event) => {
            setSnapshots(event.target.checked);
            setPicked(null);
          }}
          className="size-4 accent-silk"
        />
        {t("instances.showSnapshots")}
      </label>
      {versions.isError ? (
        <p className="m-0 text-[0.875rem] text-stale">
          {t(`errors.${toIpcError(versions.error).code}`)}
        </p>
      ) : null}
    </form>
  );
}
