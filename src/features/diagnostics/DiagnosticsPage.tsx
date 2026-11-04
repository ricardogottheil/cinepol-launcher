import { useSearch } from "@tanstack/react-router";
import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { cx } from "../../components/ui/cx";
import { Meter } from "../../components/ui/Meter";
import { SelectField } from "../../components/ui/SelectField";
import { SignalControl } from "../../components/ui/SignalControl";
import { TextField } from "../../components/ui/TextField";
import { toast } from "../../components/ui/toastStore";
import type { DiagnosticDto, HopDto } from "../../ipc/client";
import { bundledConfigDto, useServerConfig } from "../server/useServer";
import { useSavedServers } from "../servers/useSavedServers";
import { useAppInfo } from "../system/useAppInfo";
import {
  clock,
  dnsTone,
  latencyTone,
  lossTone,
  ms,
  percent,
  type Tone,
  verdictWords,
} from "./assess";
import { DURATIONS, useDiagnostics } from "./diagnosticsStore";
import { renderReport } from "./reportText";

/** The "another address" choice of the server picker. */
const OTHER = "__other__";

const LAMP_CLASS: Record<Tone, string> = {
  good: "bg-ow-foliage",
  warn: "bg-stale",
  bad: "bg-ow-roof",
  unknown: "bg-hairline-lit",
};

/** A reading's lamp; its meaning is always spelled out next to it too. */
function ToneLamp({ tone }: { tone: Tone }) {
  return (
    <span
      aria-hidden="true"
      data-tone={tone}
      className={cx("inline-block size-2 shrink-0 rounded-full", LAMP_CLASS[tone])}
    />
  );
}

function ServerPicker() {
  const { t } = useTranslation();
  const config = useServerConfig().data ?? bundledConfigDto();
  const saved = useSavedServers().data ?? [];
  const address = useDiagnostics((state) => state.address);
  const running = useDiagnostics((state) => state.running);
  const select = useDiagnostics((state) => state.select);
  const known = [
    ...config.servers.map((server) => ({ value: server.address, label: server.name })),
    ...saved
      .filter((server) => !config.servers.some((c) => c.address === server.address))
      .map((server) => ({ value: server.address, label: server.name })),
  ];
  const listed = known.some((server) => server.value === address);
  const [typing, setTyping] = useState(false);
  const other = typing || (address !== "" && !listed);

  const first = known[0]?.value;

  useEffect(() => {
    // Start on the first server, unless the player chose to type one.
    if (address === "" && !typing && first) select(first);
  }, [address, typing, first, select]);

  return (
    <div className="flex min-w-[260px] flex-1 flex-wrap items-end gap-3">
      <SelectField
        label={t("diagnostics.server")}
        options={[
          ...known.map((server) => ({
            value: server.value,
            label: `${server.label} · ${server.value}`,
          })),
          { value: OTHER, label: t("diagnostics.otherAddress") },
        ]}
        value={other ? OTHER : address}
        disabled={running}
        onChange={(event) => {
          if (event.target.value === OTHER) {
            setTyping(true);
            select("");
          } else {
            setTyping(false);
            select(event.target.value);
          }
        }}
        className="min-w-[240px] flex-1"
      />
      {other ? (
        <TextField
          label={t("diagnostics.addressLabel")}
          value={address}
          spellCheck={false}
          autoComplete="off"
          disabled={running}
          onChange={(event) => select(event.target.value)}
          className="min-w-[220px] flex-1"
        />
      ) : null}
    </div>
  );
}

function Controls() {
  const { t } = useTranslation();
  const running = useDiagnostics((state) => state.running);
  const address = useDiagnostics((state) => state.address);
  const durationSecs = useDiagnostics((state) => state.durationSecs);
  const setDuration = useDiagnostics((state) => state.setDuration);
  const start = useDiagnostics((state) => state.start);
  const stop = useDiagnostics((state) => state.stop);

  return (
    <div className="flex flex-wrap items-end gap-4">
      <ServerPicker />
      <SelectField
        label={t("diagnostics.duration")}
        options={DURATIONS.map((secs) => ({
          value: String(secs),
          label: t("diagnostics.minutes", { count: secs / 60 }),
        }))}
        value={String(durationSecs)}
        disabled={running}
        onChange={(event) => setDuration(Number(event.target.value))}
      />
      {running ? (
        <Button onClick={() => void stop()}>{t("diagnostics.stop")}</Button>
      ) : (
        <SignalControl
          label={t("diagnostics.start")}
          size="compact"
          onPress={() => {
            if (address.trim() !== "") void start();
          }}
        />
      )}
    </div>
  );
}

function Progress({ diagnostic }: { diagnostic: DiagnosticDto | null }) {
  const { t } = useTranslation();
  const running = useDiagnostics((state) => state.running);
  const durationSecs = useDiagnostics((state) => state.durationSecs);
  if (!running) return null;
  const elapsed = diagnostic?.elapsedSecs ?? 0;
  const planned = diagnostic?.plannedSecs ?? durationSecs;
  return (
    <div className="flex flex-col gap-2">
      <p className="m-0 text-silk-dim">
        {t("diagnostics.progress", { left: clock(planned - elapsed) })}
      </p>
      <Meter label={t("diagnostics.progressLabel")} value={elapsed} max={planned} format={clock} />
    </div>
  );
}

function Verdict({ diagnostic }: { diagnostic: DiagnosticDto | null }) {
  const { t } = useTranslation();
  const running = useDiagnostics((state) => state.running);
  const error = useDiagnostics((state) => state.error);
  if (error) {
    return (
      <p role="alert" className="m-0 text-stale">
        {t(`errors.${error}`)}
      </p>
    );
  }
  if (!diagnostic) {
    return (
      <section className="rounded-control border border-hairline bg-panel p-5">
        <p className="m-0 text-silk">{t(running ? "diagnostics.starting" : "diagnostics.ready")}</p>
        <p className="m-0 mt-1 text-silk-dim">{t("diagnostics.readyDetail")}</p>
      </section>
    );
  }
  const words = verdictWords(diagnostic.verdict);
  const stillMeasuring = running && diagnostic.verdict.kind !== "measuring";
  return (
    <section
      aria-live="polite"
      data-verdict={diagnostic.verdict.kind}
      className="flex flex-col gap-2 rounded-control border border-hairline-lit bg-panel p-5"
    >
      <p className="m-0 flex items-center gap-3 text-[1.125rem] text-silk">
        <ToneLamp tone={words.tone} />
        {t(words.title)}
      </p>
      {words.issue ? <p className="m-0 text-silk-dim">{t(words.issue)}</p> : null}
      {words.advice ? <p className="m-0 text-silk-dim">{t(words.advice)}</p> : null}
      {stillMeasuring ? (
        <p className="m-0 text-[0.875rem] text-engrave">{t("diagnostics.liveVerdict")}</p>
      ) : null}
      {diagnostic.hopProbingUnavailable !== null ? (
        <p className="m-0 text-[0.875rem] text-stale">{t("diagnostics.hops.unavailable")}</p>
      ) : null}
    </section>
  );
}

function Reading({
  unit,
  value,
  detail,
  tone,
}: {
  unit: string;
  value: string | null;
  detail: string;
  tone: Tone;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex min-w-[180px] flex-1 flex-col gap-1 rounded-control border border-hairline bg-panel p-4">
      <span className="plate flex items-center gap-2 text-engrave">
        <ToneLamp tone={tone} />
        {unit}
      </span>
      <span data-readout className={cx("readout-md", value ? "text-silk" : "text-engrave")}>
        {value ?? "—"}
      </span>
      <span className="text-[0.8125rem] text-silk-dim">
        {t(`diagnostics.tone.${tone}`)}
        {detail ? ` · ${detail}` : ""}
      </span>
    </div>
  );
}

function Readings({ diagnostic }: { diagnostic: DiagnosticDto | null }) {
  const { t } = useTranslation();
  const destinationJitter = diagnostic?.destinationReached
    ? diagnostic.hops.at(-1)?.stats.jitterMs
    : diagnostic?.tcp.jitterMs;
  const loss = diagnostic?.destinationLossPercent ?? 0;
  const lossSent = diagnostic?.destinationReached
    ? (diagnostic.hops.at(-1)?.stats.sent ?? 0)
    : (diagnostic?.tcp.sent ?? 0);
  const status = diagnostic?.serverStatus;
  const withUnit = (value: string | null) => (value === null ? null : `${value} ms`);
  return (
    <div className="flex flex-wrap gap-4">
      <Reading
        unit={t("diagnostics.cards.dns")}
        value={withUnit(ms(diagnostic?.dnsMs))}
        detail={diagnostic?.address ?? ""}
        tone={dnsTone(diagnostic?.dnsMs)}
      />
      <Reading
        unit={t("diagnostics.cards.tcp")}
        value={withUnit(ms(diagnostic?.tcp.averageMs))}
        detail={
          diagnostic
            ? t("diagnostics.cards.tcpDetail", { loss: percent(diagnostic.tcp.lossPercent) })
            : ""
        }
        tone={
          diagnostic && diagnostic.tcp.sent > 0 && diagnostic.tcp.received === 0
            ? "bad"
            : latencyTone(diagnostic?.tcp.averageMs)
        }
      />
      <Reading
        unit={t("diagnostics.cards.minecraft")}
        value={withUnit(ms(diagnostic?.minecraft.averageMs))}
        detail={
          status
            ? t("diagnostics.cards.players", {
                online: status.onlinePlayers,
                max: status.maxPlayers,
              })
            : ""
        }
        tone={latencyTone(diagnostic?.minecraft.averageMs)}
      />
      <Reading
        unit={t("diagnostics.cards.loss")}
        value={diagnostic && lossSent > 0 ? percent(loss) : null}
        detail={
          destinationJitter != null
            ? t("diagnostics.cards.jitter", { jitter: ms(destinationJitter) })
            : ""
        }
        tone={lossTone(loss, lossSent)}
      />
    </div>
  );
}

/** The latest samples as bars; a lost probe is a full-height bar. */
function History({ samples }: { samples: (number | null)[] }) {
  const width = 120;
  const height = 16;
  const top = Math.max(1, ...samples.map((sample) => sample ?? 0));
  const step = width / Math.max(samples.length, 1);
  return (
    <svg
      aria-hidden="true"
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      className="block"
    >
      {samples.map((sample, index) => {
        const lost = sample === null;
        const bar = lost ? height : Math.max(1, (sample / top) * height);
        return (
          <rect
            // Samples shift left as new ones arrive; position is their identity.
            // biome-ignore lint/suspicious/noArrayIndexKey: positional bars
            key={index}
            x={index * step}
            y={height - bar}
            width={Math.max(1, step - 0.5)}
            height={bar}
            className={lost ? "fill-ow-roof" : "fill-engrave"}
          />
        );
      })}
    </svg>
  );
}

function hostLabel(t: ReturnType<typeof useTranslation>["t"], hop: HopDto, showPrivate: boolean) {
  if (hop.privateToPlayer && !showPrivate) {
    return t(`diagnostics.hops.hidden.${hop.zone}` as "diagnostics.hops.hidden.home");
  }
  return hop.address ?? t("diagnostics.hops.noAnswer");
}

function HopTable({ diagnostic }: { diagnostic: DiagnosticDto }) {
  const { t } = useTranslation();
  const showPrivate = useDiagnostics((state) => state.showPrivate);
  const setShowPrivate = useDiagnostics((state) => state.setShowPrivate);
  const toggle = useId();
  if (diagnostic.hopProbingUnavailable !== null) {
    return <p className="m-0 text-silk-dim">{t("diagnostics.hops.unavailable")}</p>;
  }
  const columns = [
    "diagnostics.hops.loss",
    "diagnostics.hops.sent",
    "diagnostics.hops.last",
    "diagnostics.hops.average",
    "diagnostics.hops.best",
    "diagnostics.hops.worst",
    "diagnostics.hops.jitter",
  ] as const;
  return (
    <div className="flex flex-col gap-3">
      <label htmlFor={toggle} className="flex items-center gap-2 text-silk-dim">
        <input
          id={toggle}
          type="checkbox"
          checked={showPrivate}
          onChange={(event) => setShowPrivate(event.target.checked)}
        />
        {t("diagnostics.hops.showPrivate")}
      </label>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-left text-[0.875rem]">
          <thead>
            <tr className="plate text-engrave">
              <th scope="col" className="px-2 py-2">
                #
              </th>
              <th scope="col" className="px-2 py-2">
                {t("diagnostics.hops.host")}
              </th>
              {columns.map((column) => (
                <th key={column} scope="col" className="px-2 py-2 text-right">
                  {t(column)}
                </th>
              ))}
              <th scope="col" className="px-2 py-2">
                {t("diagnostics.hops.history")}
              </th>
            </tr>
          </thead>
          <tbody>
            {diagnostic.hops.map((hop) => {
              const tone =
                hop.stats.received === 0
                  ? "unknown"
                  : lossTone(hop.stats.lossPercent, hop.stats.sent);
              const values = [
                percent(hop.stats.lossPercent),
                String(hop.stats.sent),
                ms(hop.stats.lastMs),
                ms(hop.stats.averageMs),
                ms(hop.stats.bestMs),
                ms(hop.stats.worstMs),
                ms(hop.stats.jitterMs),
              ];
              return (
                <tr key={hop.ttl} data-hop={hop.ttl} className="border-t border-hairline">
                  <td data-readout className="readout-sm px-2 py-2 text-engrave">
                    {hop.ttl}
                  </td>
                  <td className="px-2 py-2">
                    <span
                      className={cx(
                        "readout-sm",
                        hop.privateToPlayer && !showPrivate ? "text-engrave italic" : "text-silk",
                      )}
                    >
                      {hostLabel(t, hop, showPrivate)}
                    </span>
                    <span className="block text-[0.75rem] text-engrave">
                      {t(`diagnostics.zones.${hop.zone}`)}
                    </span>
                  </td>
                  {values.map((value, index) => (
                    <td
                      key={columns[index]}
                      data-readout
                      className={cx(
                        "readout-sm px-2 py-2 text-right",
                        index === 0 && tone !== "good" && tone !== "unknown"
                          ? "text-stale"
                          : "text-silk-dim",
                      )}
                    >
                      {value ?? "—"}
                    </td>
                  ))}
                  <td className="px-2 py-2">
                    <History samples={hop.stats.history} />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="m-0 text-[0.8125rem] text-engrave">{t("diagnostics.hops.explain")}</p>
    </div>
  );
}

function Share({ diagnostic }: { diagnostic: DiagnosticDto }) {
  const { t } = useTranslation();
  const appInfo = useAppInfo().data;
  const state = useDiagnostics();
  const running = state.running;
  const report = () =>
    renderReport({
      diagnostic,
      startedAt: state.startedAt ?? new Date(),
      launcherVersion: appInfo?.version ?? null,
      showPrivate: state.showPrivate,
    });
  const copy = async (
    text: string,
    done: "diagnostics.share.copied" | "diagnostics.share.linkCopied",
  ) => {
    try {
      await navigator.clipboard.writeText(text);
      toast(done, { tone: "success" });
    } catch {
      toast("diagnostics.share.copyFailed");
    }
  };
  const expiresDays = state.shared?.expiresAt
    ? Math.max(1, Math.round((state.shared.expiresAt * 1000 - Date.now()) / 86_400_000))
    : null;

  return (
    <section className="flex flex-col gap-3 rounded-control border border-hairline bg-panel p-5">
      <h2 className="plate m-0 text-engrave">{t("diagnostics.share.title")}</h2>
      <div className="flex flex-wrap gap-3">
        <Button disabled={running || state.sharing} onClick={() => void state.share(report())}>
          {state.sharing ? t("diagnostics.share.creating") : t("diagnostics.share.createLink")}
        </Button>
        <Button disabled={running} onClick={() => void copy(report(), "diagnostics.share.copied")}>
          {t("diagnostics.share.copyText")}
        </Button>
      </div>
      {running ? (
        <p className="m-0 text-[0.8125rem] text-engrave">{t("diagnostics.share.afterTest")}</p>
      ) : null}
      {state.shared ? (
        <div className="flex flex-wrap items-center gap-3">
          <a
            href={state.shared.url}
            target="_blank"
            rel="noreferrer"
            data-readout
            className="readout-sm break-all text-silk"
          >
            {state.shared.url}
          </a>
          <Button
            onClick={() => void copy(state.shared?.url ?? "", "diagnostics.share.linkCopied")}
          >
            {t("diagnostics.share.copyLink")}
          </Button>
        </div>
      ) : null}
      {state.shareError ? (
        <p role="alert" className="m-0 text-[0.875rem] text-stale">
          {t(`errors.${state.shareError}`)}
        </p>
      ) : null}
      <p className="m-0 text-[0.8125rem] text-silk-dim">
        {state.showPrivate
          ? t("diagnostics.share.privateShown")
          : t("diagnostics.share.privateHidden")}{" "}
        {expiresDays !== null
          ? t("diagnostics.share.expiresIn", { count: expiresDays })
          : t("diagnostics.share.expires")}
      </p>
    </section>
  );
}

/**
 * Network diagnosis, MTR style: measures each hop between the player and a
 * server, says in plain words where a problem starts, and makes a report to
 * hand to the staff.
 */
export function DiagnosticsPage() {
  const { t } = useTranslation();
  const search = useSearch({ strict: false }) as { server?: string };
  const diagnostic = useDiagnostics((state) => state.diagnostic);
  const select = useDiagnostics((state) => state.select);
  const details = useId();

  useEffect(() => {
    if (search.server) select(search.server);
  }, [search.server, select]);

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-8 px-gutter-lg py-10">
      <div className="flex flex-col gap-2">
        <h1 className="display-lg m-0 text-silk">{t("diagnostics.title")}</h1>
        <p className="m-0 max-w-[70ch] text-silk-dim">{t("diagnostics.lead")}</p>
      </div>
      <Controls />
      <Progress diagnostic={diagnostic} />
      <Verdict diagnostic={diagnostic} />
      <Readings diagnostic={diagnostic} />
      {diagnostic && diagnostic.verdict.kind !== "nameNotFound" ? (
        <>
          <details className="group rounded-control border border-hairline bg-panel-deep p-5">
            <summary id={details} className="cursor-pointer text-silk">
              {t("diagnostics.hops.title", { count: diagnostic.hops.length })}
            </summary>
            <div className="mt-4">
              <HopTable diagnostic={diagnostic} />
            </div>
          </details>
          <Share diagnostic={diagnostic} />
        </>
      ) : null}
    </div>
  );
}
