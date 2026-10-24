import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import { Readout } from "../../components/ui/Readout";
import type { InstanceDto } from "../../ipc/client";
import { splitPlaytime } from "./formatPlaytime";
import { InstallProgress, InstanceStatus, PlayControl } from "./InstanceActivity";
import { useInstanceSession } from "./useInstanceSession";

export function loaderLabel(instance: InstanceDto): string {
  return instance.loaderVersion ? `${instance.loader} ${instance.loaderVersion}` : instance.loader;
}

/** One instance in a list: its spec sheet and Play. The rest is on its own page. */
export function InstanceRow({ instance }: { instance: InstanceDto }) {
  const { t } = useTranslation();
  const session = useInstanceSession(instance);

  return (
    <li className="flex flex-col gap-4 py-6">
      <div className="flex flex-wrap items-center justify-between gap-6">
        <div className="flex flex-wrap items-end gap-x-10 gap-y-3">
          <h2 className="display-md m-0 min-w-[200px]">
            <Link
              to="/instances/$instanceId"
              params={{ instanceId: instance.id }}
              className="text-silk no-underline transition-colors duration-[160ms] ease-settle hover:text-silk-dim"
            >
              {instance.name}
            </Link>
          </h2>
          <Readout unit={t("instances.spec.version")} value={instance.gameVersion} size="sm" />
          <Readout unit={t("instances.spec.loader")} value={loaderLabel(instance)} size="sm" />
          <Readout
            unit={t("instances.spec.playtime")}
            value={t("instances.playtime", splitPlaytime(instance.playtimeSecs))}
            size="sm"
          />
        </div>
        <PlayControl instance={instance} session={session} />
      </div>
      <InstallProgress session={session} />
      <InstanceStatus session={session} pendingFilesHint />
    </li>
  );
}
