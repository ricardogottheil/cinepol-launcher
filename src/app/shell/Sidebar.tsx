import { Link } from "@tanstack/react-router";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { BrandGlyph, type BrandGlyphName } from "../../components/ui/BrandGlyph";
import { ProfileSwitcher } from "../../features/accounts/ProfileSwitcher";
import { bundledConfigDto, useServerConfig } from "../../features/server/useServer";
import { openExternal } from "../../ipc/client";

type NavItem = {
  to: "/" | "/instances" | "/modpacks" | "/profile" | "/settings" | "/design";
  key:
    | "nav.home"
    | "nav.instances"
    | "nav.modpacks"
    | "nav.profile"
    | "nav.settings"
    | "nav.designKit";
};

type NavGroupKey = "navGroups.game" | "navGroups.player" | "navGroups.launcher";

const GROUPS: { key: NavGroupKey; items: NavItem[] }[] = [
  {
    key: "navGroups.game",
    items: [
      { to: "/", key: "nav.home" },
      { to: "/instances", key: "nav.instances" },
      { to: "/modpacks", key: "nav.modpacks" },
    ],
  },
  { key: "navGroups.player", items: [{ to: "/profile", key: "nav.profile" }] },
  {
    key: "navGroups.launcher",
    items: [
      { to: "/settings", key: "nav.settings" },
      ...(import.meta.env.DEV ? [{ to: "/design", key: "nav.designKit" } as const] : []),
    ],
  },
];

const itemClass =
  "nav-label flex h-11 items-center px-gutter text-engrave no-underline transition-colors duration-[160ms] ease-settle hover:text-silk-dim";

function NavGroup({ label, items }: { label: string; items: NavItem[] }) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <div className="flex flex-col pt-5 first:pt-4">
      <p id={id} className="plate m-0 px-gutter pb-2 text-engrave">
        {label}
      </p>
      <ul aria-labelledby={id} className="m-0 list-none p-0">
        {items.map((item) => (
          <li key={item.to}>
            <Link
              to={item.to}
              activeOptions={{ exact: item.to === "/" }}
              className={itemClass}
              activeProps={{
                className: "bg-panel text-silk hover:text-silk",
                "aria-current": "page",
              }}
            >
              {t(item.key)}
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * Section rail grouped by what the player is doing: playing, their profile,
 * the launcher itself. Text only, like the website's section strip; the
 * active profile sits at the foot.
 */
export function Sidebar() {
  const { t } = useTranslation();
  const [linkFailed, setLinkFailed] = useState(false);
  const { links } = useServerConfig().data ?? bundledConfigDto();

  return (
    <aside
      data-chrome
      className="flex w-sidebar shrink-0 flex-col border-r border-hairline bg-chassis"
    >
      <nav aria-label={t("chrome.mainNavigation")} className="min-h-0 overflow-y-auto">
        {GROUPS.map((group) => (
          <NavGroup key={group.key} label={t(group.key)} items={group.items} />
        ))}
      </nav>

      <div className="mt-auto flex flex-col gap-1 border-t border-hairline px-gutter py-4">
        <p className="plate m-0 mb-2 text-engrave">{t("chrome.community")}</p>
        <CommunityLink
          href={links.twitch}
          glyph="twitch"
          label={t("links.twitch")}
          onResult={setLinkFailed}
        />
        <CommunityLink
          href={links.discord}
          glyph="discord"
          label={t("links.discord")}
          onResult={setLinkFailed}
        />
        <p role="status" className="m-0 text-[0.75rem] text-stale">
          {linkFailed ? t("links.openFailed") : null}
        </p>
      </div>
      <ProfileSwitcher />
    </aside>
  );
}

/** Opens in the system browser, not inside the launcher window. */
function CommunityLink({
  href,
  glyph,
  label,
  onResult,
}: {
  href: string;
  glyph: BrandGlyphName;
  label: string;
  /** Called with `true` when the browser could not be opened. */
  onResult: (failed: boolean) => void;
}) {
  return (
    <a
      href={href}
      onClick={(event) => {
        event.preventDefault();
        openExternal(href).then(
          () => onResult(false),
          () => onResult(true),
        );
      }}
      className="flex h-9 items-center gap-3 text-[0.875rem] text-silk-dim no-underline transition-colors duration-[160ms] ease-settle hover:text-silk"
    >
      <BrandGlyph name={glyph} />
      {label}
    </a>
  );
}
