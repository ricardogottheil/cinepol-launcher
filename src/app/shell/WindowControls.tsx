import { useTranslation } from "react-i18next";
import { closeWindow, minimizeWindow, toggleMaximizeWindow } from "../../ipc/window";

const glyph = { width: 10, height: 10, viewBox: "0 0 10 10" } as const;
const stroke = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1,
  shapeRendering: "crispEdges",
} as const;

const controlClass =
  "inline-flex h-full w-12 cursor-pointer items-center justify-center text-silk-dim transition-colors duration-[160ms] ease-settle";

/** Minimize / maximize / close for Windows and Linux (macOS keeps its traffic lights). */
export function WindowControls() {
  const { t } = useTranslation();
  return (
    <div className="flex h-full" data-chrome>
      <button
        type="button"
        aria-label={t("chrome.minimize")}
        title={t("chrome.minimize")}
        onClick={() => void minimizeWindow()}
        className={`${controlClass} hover:bg-panel hover:text-silk`}
      >
        <svg {...glyph} aria-hidden="true">
          <path d="M0 5.5h10" {...stroke} />
        </svg>
      </button>
      <button
        type="button"
        aria-label={t("chrome.maximize")}
        title={t("chrome.maximize")}
        onClick={() => void toggleMaximizeWindow()}
        className={`${controlClass} hover:bg-panel hover:text-silk`}
      >
        <svg {...glyph} aria-hidden="true">
          <rect x="0.5" y="0.5" width="9" height="9" {...stroke} />
        </svg>
      </button>
      <button
        type="button"
        aria-label={t("chrome.close")}
        title={t("chrome.close")}
        onClick={() => void closeWindow()}
        className={`${controlClass} hover:bg-hairline-lit hover:text-silk`}
      >
        <svg {...glyph} aria-hidden="true">
          <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" fill="none" stroke="currentColor" strokeWidth={1} />
        </svg>
      </button>
    </div>
  );
}
