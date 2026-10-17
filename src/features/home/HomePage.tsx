import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import serverBanner from "../../assets/worlds/cinepol-recursos.webp";
import { Readout } from "../../components/ui/Readout";
import { SignalControl } from "../../components/ui/SignalControl";
import { SpecSheet } from "../../components/ui/SpecSheet";
import { type ServerStatus, StatusLamp } from "../../components/ui/StatusLamp";
import { WorldFrame } from "../../components/ui/WorldFrame";
import { bundledServerConfig, displayAddress } from "../server/serverConfig";

const plateLink =
  "plate-lg inline-flex h-11 items-center rounded-plate border border-hairline-lit px-4 text-silk-dim no-underline transition-colors duration-[160ms] ease-settle hover:border-engrave hover:text-silk";

export interface HomePageProps {
  /** Live status of the server. The Server List Ping arrives in Milestone 6. */
  status?: ServerStatus;
  onlinePlayers?: number | null;
  /** Display name of the active account, if any (Milestone 7). */
  accountName?: string | null;
}

/** Launcher home: quick play on CINEPOL, then the player's instances. */
export function HomePage({
  status = "unavailable",
  onlinePlayers = null,
  accountName = null,
}: HomePageProps) {
  const { t } = useTranslation();
  const config = bundledServerConfig;
  const { recommended } = config.versions;

  return (
    <div className="@container mx-auto flex max-w-[1120px] flex-col px-gutter-lg">
      <section
        aria-labelledby="quick-play-title"
        className="grid grid-cols-12 items-start gap-x-10 gap-y-8 border-b border-hairline py-10"
      >
        <div className="col-span-12 flex flex-col @3xl:col-span-7">
          <h1 id="quick-play-title" className="display-lg m-0 text-silk">
            {t("home.title")}
          </h1>

          <div className="mt-8 flex flex-wrap items-start gap-x-12 gap-y-6">
            <SpecSheet
              rows={[
                { label: t("home.spec.server"), value: displayAddress(config) },
                {
                  label: t("home.spec.version"),
                  value: t("home.recommended", { version: recommended }),
                },
              ]}
            />
            <div className="flex flex-col gap-3">
              <SpecSheet
                rows={[
                  { label: t("home.spec.account"), value: accountName ?? t("home.noAccount") },
                ]}
              />
            </div>
          </div>

          <div data-enter-control className="mt-8 flex flex-col gap-3">
            <SignalControl
              label={t("home.play")}
              detail={t("home.gameVersion", { version: recommended })}
            />
            {accountName ? null : (
              <Link to="/accounts" className={`${plateLink} self-start`}>
                {t("home.addAccount")}
              </Link>
            )}
          </div>
        </div>

        <div className="col-span-12 flex flex-col gap-6 @3xl:col-span-5">
          <WorldFrame
            world="overworld"
            src={serverBanner}
            caption={t("home.bannerCaption")}
            className="hidden @3xl:block"
          />
          <div
            data-live-status
            data-status={status}
            className="flex flex-wrap items-start gap-x-10 gap-y-4"
          >
            <Readout
              unit={t("home.readouts.online")}
              value={onlinePlayers === null ? null : String(onlinePlayers)}
              size="md"
              lead={<StatusLamp status={status} />}
            />
            <Readout
              unit={t("home.readouts.versions")}
              value={`${config.versions.min} – ${config.versions.max}`}
              size="md"
            />
          </div>
          {status === "online" ? null : (
            <span data-readout className="readout-sm -mt-2 text-engrave">
              {t(`home.status.${status}` as "home.status.unavailable")}
            </span>
          )}
        </div>
      </section>

      <section aria-labelledby="instances-title" className="py-10">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <h2 id="instances-title" className="display-md m-0 text-silk">
            {t("home.instances.title")}
          </h2>
          <div className="flex gap-3">
            <Link to="/modpacks" className={plateLink}>
              {t("home.instances.import")}
            </Link>
            <Link to="/instances" className={plateLink}>
              {t("home.instances.create")}
            </Link>
          </div>
        </div>
        <p className="mt-6 mb-0 border-t border-hairline pt-6 text-silk-dim">
          {t("home.instances.empty")}
        </p>
      </section>
    </div>
  );
}
