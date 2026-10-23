import { useTranslation } from "react-i18next";
import { toIpcError } from "../../ipc/client";
import { InstanceRow } from "../instances/InstanceRow";
import { useInstances } from "../instances/useInstances";

/** The instances installed from a modpack, with play and update. */
export function InstalledTab() {
  const { t } = useTranslation();
  const instances = useInstances();
  const packs = (instances.data ?? []).filter((instance) => instance.origin === "modpack");

  if (instances.isPending) return <p className="m-0 text-engrave">{t("instances.loading")}</p>;
  if (instances.isError) {
    return <p className="m-0 text-stale">{t(`errors.${toIpcError(instances.error).code}`)}</p>;
  }
  if (packs.length === 0) {
    return (
      <p className="m-0 border-t border-hairline pt-6 text-silk-dim">{t("modpacks.noInstalled")}</p>
    );
  }
  return (
    <ul className="m-0 list-none border-t border-hairline p-0">
      {packs.map((instance) => (
        <InstanceRow key={instance.id} instance={instance} />
      ))}
    </ul>
  );
}
