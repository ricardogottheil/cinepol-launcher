import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import { BrandGlyph } from "../../components/ui/BrandGlyph";
import { bundledServerConfig } from "../../features/server/serverConfig";
import { useAppInfo } from "../../features/system/useAppInfo";

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
  const { links } = bundledServerConfig;
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
        <a
          href={links.twitch}
          target="_blank"
          rel="noreferrer"
          className="flex h-9 items-center gap-3 text-[0.875rem] text-silk-dim no-underline transition-colors duration-[160ms] ease-settle hover:text-silk"
        >
          <BrandGlyph name="twitch" />
          {t("links.twitch")}
        </a>
        <a
          href={links.discord}
          target="_blank"
          rel="noreferrer"
          className="flex h-9 items-center gap-3 text-[0.875rem] text-silk-dim no-underline transition-colors duration-[160ms] ease-settle hover:text-silk"
        >
          <BrandGlyph name="discord" />
          {t("links.discord")}
        </a>
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
