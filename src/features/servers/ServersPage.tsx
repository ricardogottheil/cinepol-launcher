import { Link } from "@tanstack/react-router";
import { type FormEvent, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, plateLinkClass } from "../../components/ui/Button";
import { Readout } from "../../components/ui/Readout";
import { SelectField } from "../../components/ui/SelectField";
import { SignalControl } from "../../components/ui/SignalControl";
import { StatusLamp } from "../../components/ui/StatusLamp";
import { TextField } from "../../components/ui/TextField";
import { type CommunityServerDto, toIpcError } from "../../ipc/client";
import { CINEPOL_SESSION, useActivityStore, useInstanceActivity } from "../instances/activityStore";
import { useInstances } from "../instances/useInstances";
import { bundledConfigDto, useServerConfig } from "../server/useServer";
import { SavedServerRow, serverFormError } from "./SavedServerRow";
import { serverLamp } from "./serverLamp";
import { useSavedServers, useSaveServer, useServerPing } from "./useSavedServers";

/**
 * A community server from the config, with its status and Play (the CINEPOL
 * instance). The main server's Play leaves the pick to the backend's own copy
 * of the config.
 */
function CommunityServer({ server, main }: { server: CommunityServerDto; main: boolean }) {
  const { t } = useTranslation();
  const config = useServerConfig().data ?? bundledConfigDto();
  const status = useServerPing(server.address);
  const lamp = serverLamp(status);
  const live = status.isError ? undefined : status.data;
  const session = useInstanceActivity(CINEPOL_SESSION);
  const playCinepol = useActivityStore((state) => state.playCinepol);
  const heading = useId();

  return (
    <section
      aria-labelledby={heading}
      data-live-status
      data-status={lamp}
      className="flex flex-wrap items-center justify-between gap-x-10 gap-y-6 rounded-control border border-hairline-lit bg-panel p-6"
    >
      <div className="flex min-w-0 flex-col gap-1">
        <h2 id={heading} className="display-md m-0 break-words text-silk">
          {server.name}
        </h2>
        <span data-readout className="readout-sm break-all text-silk-dim">
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
        <Readout
          unit={t("home.readouts.versions")}
          value={`${config.minVersion} – ${config.maxVersion}`}
          size="sm"
        />
        {live?.latencyMs != null ? (
          <Readout unit={t("home.readouts.latency")} value={`${live.latencyMs} ms`} size="sm" />
        ) : null}
      </div>
      {session.activity.kind === "idle" ? (
        <SignalControl
          label={t("servers.play")}
          detail={t("home.gameVersion", { version: config.recommendedVersion })}
          size="compact"
          onPress={() => void (main ? playCinepol() : playCinepol(server.address))}
        />
      ) : (
        <div className="flex flex-wrap items-center gap-4">
          <span className="text-silk-dim">{t("servers.cinepolRunning")}</span>
          <Link to="/" className={plateLinkClass}>
            {t("servers.openHome")}
          </Link>
        </div>
      )}
    </section>
  );
}

function AddServerForm() {
  const { t } = useTranslation();
  const instances = useInstances().data ?? [];
  const save = useSaveServer();
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [instanceId, setInstanceId] = useState("");
  const heading = useId();

  function submit(event: FormEvent) {
    event.preventDefault();
    save.mutate(
      { id: null, name: name.trim(), address: address.trim(), instanceId: instanceId || null },
      {
        onSuccess: () => {
          setName("");
          setAddress("");
          setInstanceId("");
        },
      },
    );
  }

  return (
    <form
      aria-labelledby={heading}
      onSubmit={submit}
      className="flex flex-col gap-5 border-t border-hairline pt-8"
    >
      <h2 id={heading} className="display-md m-0 text-silk">
        {t("servers.addTitle")}
      </h2>
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
          hint={t("servers.addressHint")}
          value={address}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => setAddress(event.target.value)}
          className="min-w-[260px] flex-1"
        />
        <SelectField
          label={t("servers.instanceLabel")}
          options={[
            { value: "", label: t("servers.pickInstance") },
            ...instances.map((instance) => ({ value: instance.id, label: instance.name })),
          ]}
          value={instanceId}
          onChange={(event) => setInstanceId(event.target.value)}
        />
      </div>
      <div className="flex flex-wrap items-center gap-4">
        <Button
          type="submit"
          disabled={save.isPending || name.trim() === "" || address.trim() === ""}
        >
          {t("servers.add")}
        </Button>
        <p role="status" className="m-0 text-[0.875rem] text-stale">
          {save.error ? serverFormError(t, toIpcError(save.error).code) : null}
        </p>
      </div>
    </form>
  );
}

/** The community servers pinned on top, then the servers the player saved, then a form to add one. */
export function ServersPage() {
  const { t } = useTranslation();
  const configQuery = useServerConfig();
  const config = configQuery.data ?? bundledConfigDto();
  // Until the backend answers, only the main server is safe to offer: the
  // others may not be in the config the backend checks Play against.
  const community = configQuery.isPlaceholderData ? config.servers.slice(0, 1) : config.servers;
  const servers = useSavedServers();
  const instances = useInstances().data ?? [];
  const yours = useId();

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("servers.title")}</h1>
      {community.map((server, index) => (
        <CommunityServer key={server.address} server={server} main={index === 0} />
      ))}
      <section aria-labelledby={yours} className="flex flex-col gap-2">
        <h2 id={yours} className="display-md m-0 text-silk">
          {t("servers.yourServers")}
        </h2>
        {servers.isError ? (
          <p className="m-0 text-stale">{t(`errors.${toIpcError(servers.error).code}`)}</p>
        ) : !servers.data ? null : servers.data.length === 0 ? (
          <p className="m-0 border-t border-hairline pt-6 text-silk-dim">{t("servers.empty")}</p>
        ) : (
          <ul aria-labelledby={yours} className="m-0 list-none border-t border-hairline p-0">
            {servers.data.map((server) => (
              <SavedServerRow key={server.id} server={server} instances={instances} />
            ))}
          </ul>
        )}
      </section>
      <AddServerForm />
    </div>
  );
}
