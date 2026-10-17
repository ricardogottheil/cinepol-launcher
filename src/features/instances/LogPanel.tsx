import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { LogLineDto } from "../../ipc/client";

/** The game's output, newest at the bottom; follows new lines while scrolled to the end. */
export function LogPanel({ lines }: { lines: LogLineDto[] }) {
  const { t } = useTranslation();
  const scroller = useRef<HTMLDivElement>(null);
  const following = useRef(true);

  useEffect(() => {
    const element = scroller.current;
    if (element && following.current && lines.length > 0) {
      element.scrollTop = element.scrollHeight;
    }
  }, [lines]);

  return (
    <section aria-label={t("instances.logTitle")} className="flex flex-col gap-2">
      <h3 className="plate m-0 text-engrave">{t("instances.logTitle")}</h3>
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
          lines.map((line, index) => (
            <pre
              // Lines have no identity of their own; the index is stable while appending.
              // biome-ignore lint/suspicious/noArrayIndexKey: append-only log
              key={index}
              data-readout
              className={`readout-sm m-0 whitespace-pre-wrap break-all ${line.stream === "stderr" ? "text-stale" : "text-silk-dim"}`}
            >
              {line.line}
            </pre>
          ))
        )}
      </div>
    </section>
  );
}
