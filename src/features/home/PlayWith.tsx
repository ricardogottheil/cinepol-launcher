import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { SelectField } from "../../components/ui/SelectField";
import { useActivityStore } from "../instances/activityStore";
import { isCinepolInstance } from "../instances/useInstanceSession";
import { useInstances } from "../instances/useInstances";
import { useServerConfig } from "../server/useServer";

/**
 * "Play with…": joins the main community server with another of the
 * player's instances. Play itself runs the community instance, so that one
 * is not offered; with no other instance there is nothing to show.
 */
export function PlayWith() {
  const { t } = useTranslation();
  const instances = useInstances();
  const config = useServerConfig();
  const play = useActivityStore((state) => state.play);
  const [picked, setPicked] = useState<string | null>(null);

  const others = (instances.data ?? []).filter(
    (instance) => !isCinepolInstance(instance, config.data),
  );
  if (others.length === 0) return null;
  // A picked instance that was deleted meanwhile falls back to the first.
  const chosen = others.find((instance) => instance.id === picked) ?? others[0];
  if (!chosen) return null;
  const joinServer = config.data?.servers[0]?.address;

  return (
    <form
      className="flex flex-wrap items-end gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        void play(chosen.id, {
          syncPack: chosen.modpackSource === "packwiz" && chosen.updatePackOnLaunch,
          ...(joinServer ? { joinServer } : {}),
        });
      }}
    >
      <SelectField
        label={t("home.playWith")}
        options={others.map((instance) => ({
          value: instance.id,
          label: t("home.playWithOption", { name: instance.name, version: instance.gameVersion }),
        }))}
        value={chosen.id}
        onChange={(event) => setPicked(event.target.value)}
        className="min-w-[16rem]"
      />
      <Button type="submit">{t("home.playWithStart")}</Button>
    </form>
  );
}
