import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { BrandGlyph, type BrandGlyphName } from "../../components/ui/BrandGlyph";
import { bundledConfigDto, useServerConfig } from "../../features/server/useServer";
import { useAppInfo } from "../../features/system/useAppInfo";
import { openExternal } from "../../ipc/client";

const NAV = [
  { to: "/", key: "nav.home" },
  { to: "/instances", key: "nav.instances" },
  { to: "/modpacks", key: "nav.modpacks" },
  { to: "/accounts", key: "nav.accounts" },
  { to: "/settings", key: "nav.settings" },
] as const;

const itemClass =
  "nav-label flex h-11 items-center px-gutter text-engrave no-underline transition-colors duration-[160ms] ease-settle hover:text-silk-dim";

/** Section rail. Text only, like the website's section strip: no icons as decoration. */
export function Sidebar() {
  const { t } = useTranslation();
  const [linkFailed, setLinkFailed] = useState(false);
  const { links } = useServerConfig().data ?? bundledConfigDto();
  const appInfo = useAppInfo();

  return (
    <aside
      data-chrome
      className="flex w-sidebar shrink-0 flex-col border-r border-hairline bg-chassis"
    >
      <nav aria-label={t("chrome.mainNavigation")} className="pt-4">
        <ul className="m-0 list-none p-0">
          {NAV.map((item) => (
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
          {import.meta.env.DEV ? (
            <li>
              <Link
                to="/design"
                className={itemClass}
                activeProps={{ className: "bg-panel text-silk" }}
              >
                {t("nav.designKit")}
              </Link>
            </li>
          ) : null}
        </ul>
      </nav>

      <div className="mt-auto flex flex-col gap-1 border-t border-hairline px-gutter py-5">
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
        <p className="mt-4 mb-0 text-[0.75rem] leading-[1.45] text-engrave">
          {t("legal.disclaimer")}
        </p>
        {appInfo.data ? (
          <p className="plate mt-3 mb-0 flex items-baseline gap-2 text-engrave">
            {t("chrome.version")}
            <span data-readout className="readout-sm normal-case tracking-normal">
              {appInfo.data.version}
            </span>
          </p>
        ) : null}
      </div>
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
