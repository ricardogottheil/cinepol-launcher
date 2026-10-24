import { useTranslation } from "react-i18next";
import { toIpcError } from "../../ipc/client";
import { InstanceRow } from "./InstanceRow";
import { NewInstanceForm } from "./NewInstanceForm";
import { useInstances } from "./useInstances";

export function InstancesPage() {
  const { t } = useTranslation();
  const instances = useInstances();

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-10 px-gutter-lg py-10">
      <h1 className="display-lg m-0 text-silk">{t("instances.title")}</h1>
      <section aria-label={t("instances.title")}>
        {instances.isPending ? (
          <p className="m-0 text-engrave">{t("instances.loading")}</p>
        ) : instances.isError ? (
          <p className="m-0 text-stale">{t(`errors.${toIpcError(instances.error).code}`)}</p>
        ) : instances.data.length === 0 ? (
          <p className="m-0 text-silk-dim">{t("instances.empty")}</p>
        ) : (
          <ul className="m-0 list-none divide-y divide-hairline p-0">
            {instances.data.map((instance) => (
              <InstanceRow key={instance.id} instance={instance} />
            ))}
          </ul>
        )}
      </section>
      <NewInstanceForm />
    </div>
  );
}
