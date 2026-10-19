import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SelectField } from "../../components/ui/SelectField";
import { TextField } from "../../components/ui/TextField";
import type { LogLevelDto, LogLineDto } from "../../ipc/client";

type MinimumLevel = "all" | "info" | "warn" | "error";

const RANK: Record<LogLevelDto, number> = {
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  error: 4,
  fatal: 5,
};
const MINIMUM: Record<MinimumLevel, number> = { all: 0, info: 2, warn: 3, error: 4 };

/** Plain lines have no level: stdout reads as info, stderr as an error. */
function rank(line: LogLineDto): number {
  if (line.level) return RANK[line.level];
  return line.stream === "stderr" ? RANK.error : RANK.info;
}

function matches(line: LogLineDto, query: string): boolean {
  if (query === "") return true;
  return [line.line, line.thread, line.logger].some((field) =>
    field?.toLowerCase().includes(query),
  );
}

/**
 * The game's output, newest at the bottom; follows new lines while scrolled
 * to the end. log4j lines show their level and thread, and the list can be
 * narrowed by minimum level and by text.
 */
export function LogPanel({ lines }: { lines: LogLineDto[] }) {
  const { t } = useTranslation();
  const scroller = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const [minimum, setMinimum] = useState<MinimumLevel>("all");
  const [search, setSearch] = useState("");
  const query = search.trim().toLowerCase();

  const visible = useMemo(
    () => lines.filter((line) => rank(line) >= MINIMUM[minimum] && matches(line, query)),
    [lines, minimum, query],
  );

  useEffect(() => {
    const element = scroller.current;
    if (element && following.current && visible.length > 0) {
      element.scrollTop = element.scrollHeight;
    }
  }, [visible]);

  const levelOptions: Array<{ value: MinimumLevel; label: string }> = [
    { value: "all", label: t("instances.log.all") },
    { value: "info", label: t("instances.log.info") },
    { value: "warn", label: t("instances.log.warn") },
    { value: "error", label: t("instances.log.error") },
  ];

  return (
    <section aria-label={t("instances.logTitle")} className="flex flex-col gap-3">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <h3 className="plate m-0 text-engrave">{t("instances.logTitle")}</h3>
        <div className="flex flex-wrap items-end gap-4">
          <SelectField
            label={t("instances.log.level")}
            options={levelOptions}
            value={minimum}
            onChange={(event) =>
              setMinimum(
                levelOptions.find((option) => option.value === event.target.value)?.value ?? "all",
              )
            }
          />
          <TextField
            label={t("instances.log.search")}
            type="search"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
        </div>
      </div>
      <div
        ref={scroller}
        role="log"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrollable region must be focusable so keyboard users can scroll it (WCAG 2.1.1).
        tabIndex={0}
        onScroll={(event) => {
          const element = event.currentTarget;
          following.current = element.scrollTop + element.clientHeight >= element.scrollHeight - 8;
        }}
        className="h-64 overflow-auto rounded-plate border border-hairline bg-panel-deep p-3"
      >
        {lines.length === 0 ? (
          <p className="m-0 text-[0.875rem] text-engrave">{t("instances.logEmpty")}</p>
        ) : (
          visible.map((line, index) => (
            <pre
              // Lines have no identity of their own; the index is stable while appending.
              // biome-ignore lint/suspicious/noArrayIndexKey: append-only log
              key={index}
              data-readout
              data-testid="log-line"
              className={`readout-sm m-0 whitespace-pre-wrap break-all ${rank(line) >= RANK.warn ? "text-stale" : "text-silk-dim"}`}
            >
              {line.level ? (
                <span className="text-engrave">
                  [{line.thread ?? "?"}/{line.level.toUpperCase()}]{" "}
                </span>
              ) : null}
              {line.line}
            </pre>
          ))
        )}
      </div>
      {lines.length > 0 ? (
        <p className="m-0 text-[0.875rem] text-engrave">
          {t("instances.log.count", { shown: visible.length, total: lines.length })}
        </p>
      ) : null}
    </section>
  );
}
