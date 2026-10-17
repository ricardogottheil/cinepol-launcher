import { type CSSProperties, useState } from "react";
import { useTranslation } from "react-i18next";
import resourceWorld from "../../assets/worlds/cinepol-recursos.webp";
import { Button } from "../../components/ui/Button";
import { Readout } from "../../components/ui/Readout";
import { SignalControl } from "../../components/ui/SignalControl";
import { type ServerStatus, StatusLamp } from "../../components/ui/StatusLamp";
import { WorldFrame } from "../../components/ui/WorldFrame";
import { bundledServerConfig, displayAddress } from "../server/serverConfig";

const COPIED_RESET_MS = 2000;

export interface HomePageProps {
  /** Live status of the server. The Server List Ping arrives in Milestone 6. */
  status?: ServerStatus;
  onlinePlayers?: number | null;
}

export function HomePage({ status = "unavailable", onlinePlayers = null }: HomePageProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const config = bundledServerConfig;
  const address = displayAddress(config);

  async function copyAddress() {
    try {
      await navigator.clipboard.writeText(address);
      setCopied(true);
      window.setTimeout(() => setCopied(false), COPIED_RESET_MS);
    } catch {
      setCopied(false);
    }
  }

  const statusNote =
    status === "online" ? null : t(`home.status.${status}` as "home.status.unavailable");

  return (
    <div className="@container mx-auto flex max-w-[1120px] flex-col px-gutter-lg">
      <section className="grid grid-cols-12 items-center gap-x-10 gap-y-10 border-b border-hairline py-12">
        <div className="col-span-12 flex flex-col @3xl:col-span-7 @6xl:col-span-6">
          <h1
            className="display-xl m-0 text-silk"
            data-enter
            style={{ "--enter-index": 1 } as CSSProperties}
          >
            {t("home.title")}
          </h1>
          <p className="measure mt-6 mb-0 text-silk-dim">{t("home.lead")}</p>

          <hr data-enter-rule className="my-8 h-px w-full border-0 bg-hairline" />

          <div
            data-live-status
            data-status={status}
            className="flex flex-wrap items-start gap-x-12 gap-y-6"
          >
            <Readout
              unit={t("home.readouts.online")}
              value={onlinePlayers === null ? null : String(onlinePlayers)}
              size="xl"
              lead={<StatusLamp status={status} />}
            />
            <Readout
              unit={t("home.readouts.versions")}
              value={`${config.versions.min} – ${config.versions.max}`}
              size="md"
            />
            <Readout
              unit={t("home.readouts.recommended")}
              value={config.versions.recommended}
              size="md"
            />
          </div>

          <div data-enter-control className="mt-8 flex flex-col gap-3">
            <SignalControl label={t("home.play")} detail={address} />
            <div className="flex items-center justify-between gap-4">
              <span data-readout className="readout-sm text-engrave">
                {statusNote}
              </span>
              <Button onClick={() => void copyAddress()}>
                {copied ? t("home.copied") : t("home.copyAddress")}
              </Button>
            </div>
            <span className="sr-only" aria-live="polite">
              {copied ? t("home.copiedAnnouncement") : ""}
            </span>
          </div>
        </div>

        <div className="col-span-12 @3xl:col-span-5 @6xl:col-span-6">
          <WorldFrame world="overworld" src={resourceWorld} caption={t("home.worldCaption")} />
        </div>
      </section>

      <section className="py-12">
        <h2 className="display-md m-0 text-silk">{t("home.instances.title")}</h2>
        <p className="mt-4 mb-0 border-t border-hairline pt-6 text-silk-dim">
          {t("home.instances.empty")}
        </p>
      </section>
    </div>
  );
}
