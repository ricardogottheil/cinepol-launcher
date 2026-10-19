import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { ConfirmDialog } from "../../components/ui/ConfirmDialog";
import { SelectField } from "../../components/ui/SelectField";
import { TextAreaField } from "../../components/ui/TextAreaField";
import { TextField } from "../../components/ui/TextField";
import {
  type InstanceDto,
  type InstanceSettingsDto,
  type JavaKindDto,
  pickJavaExecutable,
  toIpcError,
} from "../../ipc/client";
import { useAccounts } from "../accounts/useAccounts";
import {
  copyName,
  MAX_INSTANCE_NAME,
  type SettingsField,
  type SettingsFormValues,
  toFormValues,
  toSettings,
} from "./settingsForm";
import {
  useDeleteInstance,
  useDuplicateInstance,
  useInstanceSettings,
  useUpdateInstanceSettings,
} from "./useInstances";

/** Settings of one instance plus its management actions (duplicate, delete). */
export function InstanceSettingsPanel({
  instance,
  busy,
}: {
  instance: InstanceDto;
  /** The instance is running, installing, updating or exporting. */
  busy: boolean;
}) {
  const { t } = useTranslation();
  const settings = useInstanceSettings(instance.id);

  return (
    <section
      aria-label={t("instances.settings.title")}
      className="flex flex-col gap-6 border-t border-hairline pt-6"
    >
      <h3 className="plate m-0 text-engrave">{t("instances.settings.title")}</h3>
      {settings.isPending ? (
        <p className="m-0 text-engrave">{t("instances.loading")}</p>
      ) : settings.isError ? (
        <p className="m-0 text-stale">{t(`errors.${toIpcError(settings.error).code}`)}</p>
      ) : (
        <SettingsForm
          instanceId={instance.id}
          stored={settings.data}
          packwiz={instance.modpackSource === "packwiz"}
        />
      )}
      <Management instance={instance} busy={busy} />
    </section>
  );
}

function SettingsForm({
  instanceId,
  stored,
  packwiz,
}: {
  instanceId: string;
  stored: InstanceSettingsDto;
  /** Only packwiz instances can sync their pack before launching. */
  packwiz: boolean;
}) {
  const { t } = useTranslation();
  const [values, setValues] = useState<SettingsFormValues>(() => toFormValues(stored));
  const [invalid, setInvalid] = useState<{ field: SettingsField; line?: number } | null>(null);
  const [javaError, setJavaError] = useState<string | null>(null);
  const accounts = useAccounts();
  const save = useUpdateInstanceSettings(instanceId);

  const set = (patch: Partial<SettingsFormValues>) => {
    setValues((current) => ({ ...current, ...patch }));
    setInvalid(null);
    save.reset();
  };
  const fieldError = (field: SettingsField) =>
    invalid?.field !== field
      ? undefined
      : field === "env"
        ? t("instances.settings.envInvalid", { line: invalid.line })
        : t("instances.settings.numberInvalid");

  async function pickJava() {
    setJavaError(null);
    try {
      const path = await pickJavaExecutable();
      if (path) set({ javaPath: path });
    } catch (error) {
      setJavaError(t(`errors.${toIpcError(error).code}`));
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    const result = toSettings(values);
    if (!result.ok) {
      setInvalid(result.error);
      return;
    }
    save.mutate(result.settings);
  }

  const accountOptions = [
    { value: "", label: t("instances.settings.accountActive") },
    ...(accounts.data?.accounts ?? []).map((account) => ({
      value: account.id,
      label: account.username,
    })),
  ];
  const javaOptions: Array<{ value: JavaKindDto; label: string }> = [
    { value: "managed", label: t("instances.settings.javaManaged") },
    { value: "custom", label: t("instances.settings.javaCustom") },
  ];

  return (
    <form onSubmit={submit} className="flex flex-col gap-5">
      <div className="flex flex-wrap items-start gap-4">
        <TextField
          label={t("instances.nameLabel")}
          value={values.name}
          maxLength={MAX_INSTANCE_NAME}
          onChange={(event) => set({ name: event.target.value })}
        />
        <SelectField
          label={t("instances.settings.account")}
          options={accountOptions}
          value={values.defaultAccountId}
          onChange={(event) => set({ defaultAccountId: event.target.value })}
        />
      </div>

      <div className="flex flex-wrap items-end gap-4">
        <SelectField
          label={t("instances.settings.java")}
          options={javaOptions}
          value={values.javaKind}
          onChange={(event) =>
            set({ javaKind: event.target.value === "custom" ? "custom" : "managed" })
          }
        />
        {values.javaKind === "custom" ? (
          <div className="flex flex-col gap-2">
            <Button onClick={() => void pickJava()}>{t("instances.settings.pickJava")}</Button>
          </div>
        ) : null}
      </div>
      {values.javaKind === "custom" ? (
        <p className="m-0 flex flex-col gap-1 text-[0.875rem]">
          <span className="readout-sm break-all text-silk">
            {values.javaPath ?? t("instances.settings.javaNone")}
          </span>
          <span className={javaError ? "text-stale" : "text-engrave"}>
            {javaError ?? t("instances.settings.javaHint")}
          </span>
        </p>
      ) : null}

      <div className="flex flex-wrap items-start gap-4">
        <TextField
          label={t("instances.settings.minMemory")}
          inputMode="numeric"
          value={values.minMemory}
          error={fieldError("minMemory")}
          onChange={(event) => set({ minMemory: event.target.value })}
        />
        <TextField
          label={t("instances.settings.maxMemory")}
          inputMode="numeric"
          value={values.maxMemory}
          error={fieldError("maxMemory")}
          onChange={(event) => set({ maxMemory: event.target.value })}
        />
      </div>

      {packwiz ? (
        <label className="flex items-center gap-3 text-silk-dim">
          <input
            type="checkbox"
            checked={values.updatePackOnLaunch}
            onChange={(event) => set({ updatePackOnLaunch: event.target.checked })}
          />
          {t("instances.settings.updateOnLaunch")}
        </label>
      ) : null}

      <div className="flex flex-col gap-4">
        <label className="flex items-center gap-3 text-silk-dim">
          <input
            type="checkbox"
            checked={values.customWindow}
            onChange={(event) => set({ customWindow: event.target.checked })}
          />
          {t("instances.settings.customWindow")}
        </label>
        {values.customWindow ? (
          <div className="flex flex-wrap items-start gap-4">
            <TextField
              label={t("instances.settings.width")}
              inputMode="numeric"
              value={values.width}
              error={fieldError("width")}
              onChange={(event) => set({ width: event.target.value })}
            />
            <TextField
              label={t("instances.settings.height")}
              inputMode="numeric"
              value={values.height}
              error={fieldError("height")}
              onChange={(event) => set({ height: event.target.value })}
            />
          </div>
        ) : null}
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <TextAreaField
          label={t("instances.settings.jvmArgs")}
          hint={t("instances.settings.jvmArgsHint")}
          value={values.jvmArgs}
          onChange={(event) => set({ jvmArgs: event.target.value })}
        />
        <TextAreaField
          label={t("instances.settings.env")}
          hint={t("instances.settings.envHint")}
          value={values.env}
          error={fieldError("env")}
          onChange={(event) => set({ env: event.target.value })}
        />
      </div>

      <div className="flex flex-wrap items-center gap-4">
        <Button type="submit" disabled={save.isPending}>
          {t("instances.settings.save")}
        </Button>
        <p role="status" className="m-0 text-[0.875rem]">
          {save.isError ? (
            <span className="text-stale">{t(`errors.${toIpcError(save.error).code}`)}</span>
          ) : save.isSuccess ? (
            <span className="text-silk-dim">{t("instances.settings.saved")}</span>
          ) : null}
        </p>
      </div>
    </form>
  );
}

function Management({ instance, busy }: { instance: InstanceDto; busy: boolean }) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const duplicate = useDuplicateInstance(instance.id);
  const remove = useDeleteInstance(instance.id);
  const error = duplicate.error ?? remove.error;

  return (
    <div className="flex flex-col gap-3 border-t border-hairline pt-5">
      <h3 className="plate m-0 text-engrave">{t("instances.settings.manage")}</h3>
      <div className="flex flex-wrap gap-3">
        <Button
          disabled={busy || duplicate.isPending}
          onClick={() => {
            remove.reset();
            duplicate.mutate(
              copyName(instance.name, (name) => t("instances.settings.copyName", { name })),
            );
          }}
        >
          {t("instances.settings.duplicate")}
        </Button>
        <Button
          disabled={busy || remove.isPending}
          className="border-hazard text-hazard hover:text-hazard"
          onClick={() => {
            duplicate.reset();
            setConfirming(true);
          }}
        >
          {t("instances.settings.delete")}
        </Button>
      </div>
      <p role="status" className="m-0 text-[0.875rem]">
        {error ? (
          <span className="text-stale">{t(`errors.${toIpcError(error).code}`)}</span>
        ) : duplicate.data ? (
          <span className="text-silk-dim">
            {t("instances.settings.duplicated", { name: duplicate.data.name })}
          </span>
        ) : null}
      </p>
      {confirming ? (
        <ConfirmDialog
          destructive
          title={t("instances.settings.deleteTitle", { name: instance.name })}
          confirmLabel={t("instances.settings.deleteConfirm")}
          cancelLabel={t("instances.cancel")}
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            remove.mutate();
          }}
        >
          <p className="m-0">{t("instances.settings.deleteBody")}</p>
        </ConfirmDialog>
      ) : null}
    </div>
  );
}
