import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { SelectField } from "../../components/ui/SelectField";
import { TextField } from "../../components/ui/TextField";
import { type ServerDto, toIpcError } from "../../ipc/client";
import { useEditServers, useServers } from "./useInstances";

type Textures = "ask" | "on" | "off";

const toTextures = (value: boolean | null): Textures =>
  value === null ? "ask" : value ? "on" : "off";
const fromTextures = (value: Textures): boolean | null => (value === "ask" ? null : value === "on");

interface Draft {
  name: string;
  address: string;
  textures: Textures;
}

const EMPTY: Draft = { name: "", address: "", textures: "ask" };

/** An instance's `servers.dat`: list, add, edit, reorder and remove servers. */
export function ServerListPanel({ instanceId }: { instanceId: string }) {
  const { t } = useTranslation();
  const servers = useServers(instanceId);
  const edit = useEditServers(instanceId);
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [editing, setEditing] = useState<number | null>(null);
  const list = servers.data?.servers ?? [];
  // Positions must not move under an open edit, or Save would hit another server.
  const locked = edit.isPending || editing !== null;
  // Content keys: the same server can be listed twice, so repeats are numbered.
  const seen = new Map<string, number>();
  const keys = list.map((server) => {
    const base = `${server.name}\u0000${server.address}`;
    const repeat = seen.get(base) ?? 0;
    seen.set(base, repeat + 1);
    return `${base}\u0000${repeat}`;
  });

  function startEdit(index: number, server: ServerDto) {
    setEditing(index);
    setDraft({
      name: server.name,
      address: server.address,
      textures: toTextures(server.acceptTextures),
    });
    edit.reset();
  }

  function stopEdit() {
    setEditing(null);
    setDraft(EMPTY);
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    const form = {
      name: draft.name.trim(),
      address: draft.address.trim(),
      acceptTextures: fromTextures(draft.textures),
    };
    edit.mutate(
      editing === null ? { kind: "add", form } : { kind: "update", index: editing, form },
      { onSuccess: stopEdit },
    );
  }

  const textureOptions: Array<{ value: Textures; label: string }> = [
    { value: "ask", label: t("instances.servers.texturesAsk") },
    { value: "on", label: t("instances.servers.texturesOn") },
    { value: "off", label: t("instances.servers.texturesOff") },
  ];

  return (
    <section aria-label={t("instances.servers.title")} className="flex flex-col gap-5">
      {servers.isPending ? (
        <p className="m-0 text-engrave">{t("instances.loading")}</p>
      ) : servers.isError ? (
        <p className="m-0 text-stale">{t(`errors.${toIpcError(servers.error).code}`)}</p>
      ) : list.length === 0 ? (
        <p className="m-0 text-silk-dim">{t("instances.servers.empty")}</p>
      ) : (
        <ul className="m-0 list-none divide-y divide-hairline p-0">
          {list.map((server, index) => (
            <li
              key={keys[index]}
              className="flex flex-wrap items-center justify-between gap-4 py-3"
            >
              <div className="flex min-w-0 items-center gap-4">
                {server.icon ? (
                  <img
                    src={server.icon}
                    alt=""
                    width={32}
                    height={32}
                    className="size-8 shrink-0 rounded-plate"
                  />
                ) : (
                  <span
                    aria-hidden
                    className="size-8 shrink-0 rounded-plate border border-hairline"
                  />
                )}
                <div className="flex min-w-0 flex-col gap-1">
                  <span className="text-silk">{server.name}</span>
                  <span className="readout-sm break-all text-engrave">{server.address}</span>
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button
                  disabled={locked || index === 0}
                  onClick={() => edit.mutate({ kind: "move", from: index, to: index - 1 })}
                >
                  {t("instances.servers.moveUp")}
                </Button>
                <Button
                  disabled={locked || index === list.length - 1}
                  onClick={() => edit.mutate({ kind: "move", from: index, to: index + 1 })}
                >
                  {t("instances.servers.moveDown")}
                </Button>
                <Button disabled={edit.isPending} onClick={() => startEdit(index, server)}>
                  {t("instances.servers.edit")}
                </Button>
                <Button disabled={locked} onClick={() => edit.mutate({ kind: "remove", index })}>
                  {t("instances.servers.remove")}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}

      <form onSubmit={submit} className="flex flex-wrap items-end gap-4">
        <TextField
          label={t("instances.servers.name")}
          value={draft.name}
          maxLength={64}
          onChange={(event) => setDraft({ ...draft, name: event.target.value })}
        />
        <TextField
          label={t("instances.servers.address")}
          value={draft.address}
          placeholder="mc.policarpiers.com"
          onChange={(event) => setDraft({ ...draft, address: event.target.value })}
        />
        <SelectField
          label={t("instances.servers.textures")}
          options={textureOptions}
          value={draft.textures}
          onChange={(event) =>
            setDraft({
              ...draft,
              textures: textureOptions.find((option) => option.value === event.target.value)
                ? (event.target.value as Textures)
                : "ask",
            })
          }
        />
        <Button
          type="submit"
          disabled={edit.isPending || draft.name.trim() === "" || draft.address.trim() === ""}
        >
          {editing === null ? t("instances.servers.add") : t("instances.servers.save")}
        </Button>
        {editing === null ? null : (
          <Button onClick={stopEdit}>{t("instances.servers.cancel")}</Button>
        )}
      </form>
      <p role="status" className="m-0 text-[0.875rem]">
        {edit.isError ? (
          <span className="text-stale">{t(`errors.${toIpcError(edit.error).code}`)}</span>
        ) : (
          <span className="text-engrave">{t("instances.servers.hint")}</span>
        )}
      </p>
    </section>
  );
}
