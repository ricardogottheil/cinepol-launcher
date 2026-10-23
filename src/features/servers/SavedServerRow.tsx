import { type FormEvent, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { Readout } from "../../components/ui/Readout";
import { SelectField } from "../../components/ui/SelectField";
import { StatusLamp } from "../../components/ui/StatusLamp";
import { TextField } from "../../components/ui/TextField";
import { type InstanceDto, type SavedServerDto, toIpcError } from "../../ipc/client";
import { useActivityStore, useInstanceActivity } from "../instances/activityStore";
import { serverLamp } from "./serverLamp";
import { useRemoveSavedServer, useSaveServer, useServerPing } from "./useSavedServers";

/** A saved server: its live status, the instance it plays with, and its actions. */
export function SavedServerRow({
  server,
  instances,
}: {
  server: SavedServerDto;
  instances: InstanceDto[];
}) {
  const { t } = useTranslation();
  const ping = useServerPing(server.address);
  const lamp = serverLamp(ping);
  const live = ping.isError ? undefined : ping.data;
  const save = useSaveServer();
  const remove = useRemoveSavedServer();
  const play = useActivityStore((state) => state.play);
  const stop = useActivityStore((state) => state.stop);
  // A pick that points to a deleted instance counts as none.
  const instance = instances.find((candidate) => candidate.id === server.instanceId);
  const activity = useInstanceActivity(instance?.id ?? "");
  const busy = instance !== undefined && activity.activity.kind !== "idle";
  const [needsInstance, setNeedsInstance] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [editing, setEditing] = useState(false);
  const picker = useRef<HTMLDivElement>(null);
  const heading = useId();

  function onPlay() {
    if (!instance) {
      setNeedsInstance(true);
      picker.current?.querySelector("select")?.focus();
      return;
    }
    void play(instance.id, { joinServer: server.address });
  }

  const error = save.error ?? remove.error;

  return (
    <li aria-labelledby={heading} className="flex flex-col gap-5 border-b border-hairline py-6">
      {editing ? (
        <EditServer server={server} onDone={() => setEditing(false)} />
      ) : (
        <div className="flex flex-wrap items-start justify-between gap-x-10 gap-y-5">
          <div className="flex min-w-0 flex-col gap-1">
            <h3 id={heading} className="display-md m-0 break-words text-silk">
              {server.name}
            </h3>
            <span data-readout className="readout-sm text-silk-dim">
              {server.address}
            </span>
          </div>
          <div className="flex flex-wrap items-start gap-x-10 gap-y-4">
            <Readout
              unit={t("home.readouts.online")}
              value={live ? `${live.onlinePlayers} / ${live.maxPlayers}` : null}
              size="sm"
              lead={<StatusLamp status={lamp} />}
            />
            {live ? (
              <Readout unit={t("home.spec.version")} value={live.versionName} size="sm" />
            ) : (
              <span data-readout className="readout-sm self-end text-engrave">
                {t(`home.status.${lamp}` as "home.status.unavailable")}
              </span>
            )}
            {live?.latencyMs != null ? (
              <Readout unit={t("home.readouts.latency")} value={`${live.latencyMs} ms`} size="sm" />
            ) : null}
          </div>
        </div>
      )}

      <div className="flex flex-wrap items-end gap-3">
        <div ref={picker}>
          <SelectField
            label={t("servers.instanceLabel")}
            options={[
              { value: "", label: t("servers.pickInstance") },
              ...instances.map((candidate) => ({ value: candidate.id, label: candidate.name })),
            ]}
            value={instance?.id ?? ""}
            disabled={save.isPending || busy}
            onChange={(event) => {
              setNeedsInstance(false);
              save.mutate({ ...server, instanceId: event.target.value || null });
            }}
          />
        </div>
        {busy ? (
          <>
            <span className="plate h-11 content-center text-silk">{t("servers.playing")}</span>
            <Button onClick={() => instance && void stop(instance.id)}>{t("servers.stop")}</Button>
          </>
        ) : (
          <Button onClick={onPlay}>{t("servers.play")}</Button>
        )}
        {editing ? null : (
          <Button onClick={() => setEditing(true)} disabled={busy}>
            {t("servers.edit")}
          </Button>
        )}
        {confirming ? (
          <>
            <Button onClick={() => remove.mutate(server.id)} disabled={remove.isPending}>
              {t("servers.confirmRemove")}
            </Button>
            <Button onClick={() => setConfirming(false)}>{t("servers.keep")}</Button>
          </>
        ) : (
          <Button onClick={() => setConfirming(true)} disabled={busy}>
            {t("servers.remove")}
          </Button>
        )}
      </div>

      <p role="status" className="m-0 text-[0.875rem] text-stale empty:hidden">
        {needsInstance
          ? t("servers.needsInstance")
          : activity.error && instance
            ? t(`errors.${activity.error}`)
            : error
              ? t(`errors.${toIpcError(error).code}`)
              : null}
      </p>
    </li>
  );
}

function EditServer({ server, onDone }: { server: SavedServerDto; onDone: () => void }) {
  const { t } = useTranslation();
  const save = useSaveServer();
  const [name, setName] = useState(server.name);
  const [address, setAddress] = useState(server.address);

  function submit(event: FormEvent) {
    event.preventDefault();
    save.mutate({ ...server, name: name.trim(), address: address.trim() }, { onSuccess: onDone });
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-4">
      <div className="flex flex-wrap items-start gap-4">
        <TextField
          label={t("servers.nameLabel")}
          value={name}
          maxLength={64}
          onChange={(event) => setName(event.target.value)}
          className="min-w-[220px] flex-1"
        />
        <TextField
          label={t("servers.addressLabel")}
          value={address}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => setAddress(event.target.value)}
          className="min-w-[260px] flex-1"
        />
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <Button type="submit" disabled={save.isPending}>
          {t("servers.save")}
        </Button>
        <Button onClick={onDone}>{t("servers.cancel")}</Button>
        <p role="status" className="m-0 text-[0.875rem] text-stale">
          {save.error ? serverFormError(t, toIpcError(save.error).code) : null}
        </p>
      </div>
    </form>
  );
}

/** What went wrong saving a server, in the player's words. */
export function serverFormError(t: ReturnType<typeof useTranslation>["t"], code: string): string {
  if (code === "already_exists") return t("servers.alreadySaved");
  if (code === "invalid_input") return t("servers.invalidServer");
  return t(`errors.${code}` as "errors.internal");
}
