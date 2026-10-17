import { useTranslation } from "react-i18next";
import endWorld from "../../assets/worlds/cinepol-end.webp";
import netherWorld from "../../assets/worlds/cinepol-nether.webp";
import overworld from "../../assets/worlds/cinepol-parcelas.webp";
import { Button } from "../../components/ui/Button";
import { IconButton } from "../../components/ui/IconButton";
import { Readout } from "../../components/ui/Readout";
import { SignalControl } from "../../components/ui/SignalControl";
import { SpecSheet } from "../../components/ui/SpecSheet";
import { StatusLamp } from "../../components/ui/StatusLamp";
import { StepList } from "../../components/ui/StepList";
import { WorldFrame } from "../../components/ui/WorldFrame";

/** Development-only catalog of the base components (route `/design`). */
export function DesignKitPage() {
  const { t } = useTranslation();
  const s = (key: string) => t(`designKit.sample.${key}` as "designKit.sample.version");
  const step = (key: string) => t(`designKit.sampleSteps.${key}` as "designKit.sampleSteps.open");

  return (
    <div className="mx-auto flex max-w-[1120px] flex-col gap-12 px-gutter-lg py-12">
      <header>
        <h1 className="display-lg m-0 text-silk">{t("designKit.title")}</h1>
        <p className="measure mt-4 mb-0 text-silk-dim">{t("designKit.lead")}</p>
      </header>

      <section className="flex flex-col gap-6 border-t border-hairline pt-8">
        <h2 className="display-md m-0">{t("designKit.worlds")}</h2>
        <div className="grid grid-cols-3 gap-6">
          <WorldFrame world="overworld" src={overworld} caption="Overworld" />
          <WorldFrame world="nether" src={netherWorld} caption="Nether" />
          <WorldFrame world="end" src={endWorld} caption="End" />
        </div>
        <div className="grid grid-cols-3">
          {(
            [
              ["bg-ow-ground", "text-ow-sky", "Overworld"],
              ["bg-nt-ground", "text-nt-lava", "Nether"],
              ["bg-end-ground", "text-end-stone", "End"],
            ] as const
          ).map(([ground, ink, name]) => (
            <div key={name} className={`${ground} flex flex-col gap-2 px-6 py-8`}>
              <span className="display-md text-silk">{name}</span>
              <span data-readout className={`readout-sm ${ink}`}>
                {name === "Overworld" ? "5600 K" : name === "Nether" ? "2000 K" : "7500 K"}
              </span>
            </div>
          ))}
        </div>
      </section>

      <section className="flex flex-col gap-6 border-t border-hairline pt-8">
        <h2 className="display-md m-0">{t("designKit.controls")}</h2>
        <div className="max-w-[520px]">
          <SignalControl label={t("home.play")} detail="mc.policarpiers.com" />
        </div>
        <div className="flex flex-wrap items-center gap-4">
          <SignalControl label={t("home.copyAddress")} size="compact" />
          <Button>{s("secondary")}</Button>
          <Button disabled>{s("disabled")}</Button>
          <IconButton label={t("chrome.close")}>
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" fill="none" stroke="currentColor" />
            </svg>
          </IconButton>
        </div>
      </section>

      <section className="grid grid-cols-2 gap-12 border-t border-hairline pt-8">
        <div className="flex flex-col gap-6">
          <h2 className="display-md m-0">{t("designKit.readouts")}</h2>
          <div data-live-status data-status="online" className="flex gap-12">
            <Readout
              unit={t("home.readouts.online")}
              value="8"
              size="xl"
              lead={<StatusLamp status="online" />}
            />
            <Readout unit={t("home.readouts.versions")} value={"1.21.11 – 26.2"} />
            <Readout unit={t("home.readouts.recommended")} value={null} />
          </div>
        </div>
        <div className="flex flex-col gap-6">
          <h2 className="display-md m-0">{t("designKit.specSheet")}</h2>
          <SpecSheet
            rows={[
              { label: s("version"), value: "26.2" },
              { label: s("loader"), value: "Fabric 0.17.2" },
              { label: s("mods"), value: "42" },
              { label: s("java"), value: "21.0.8" },
              { label: s("memory"), value: "4 GB" },
              { label: s("playtime"), value: null },
            ]}
          />
        </div>
      </section>

      <section className="flex flex-col gap-6 border-t border-hairline pt-8">
        <h2 className="display-md m-0">{t("designKit.steps")}</h2>
        <StepList
          steps={[
            { title: step("open"), body: step("openBody") },
            { title: step("code"), body: step("codeBody") },
            { title: step("done"), body: step("doneBody") },
          ]}
        />
      </section>
    </div>
  );
}
