import { useTranslation } from "react-i18next";
import { Meter } from "../../components/ui/Meter";
import type { ProgressUnitDto } from "../../ipc/client";
import { formatBytes } from "./formatBytes";

/** A meter that reads downloads in megabytes and other stages as plain counts. */
export function ProgressMeter({
  label,
  done,
  total,
  unit,
}: {
  label: string;
  done: number;
  total: number | null;
  unit: ProgressUnitDto;
}) {
  const { t } = useTranslation();
  return unit === "bytes" ? (
    <Meter
      label={label}
      value={done}
      max={total}
      format={formatBytes}
      unitLabel={t("units.megabytes")}
    />
  ) : (
    <Meter label={label} value={done} max={total} />
  );
}
