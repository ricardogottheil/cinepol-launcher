import { cx } from "../../components/ui/cx";
import { Wordmark } from "../../components/ui/Wordmark";
import type { DesktopPlatform } from "../../ipc/window";
import { WindowControls } from "./WindowControls";

/**
 * Custom title bar. The whole bar is a drag region; on macOS the native
 * traffic lights overlay its left edge, so the wordmark clears them.
 */
export function TitleBar({ platform }: { platform: DesktopPlatform }) {
  return (
    <header
      data-tauri-drag-region
      data-chrome
      className="flex h-titlebar shrink-0 items-center border-b border-hairline bg-chassis"
    >
      <div
        data-tauri-drag-region
        className={cx(
          "flex h-full flex-1 items-center",
          platform === "macos" ? "pl-[84px]" : "pl-4",
        )}
      >
        <Wordmark className="pointer-events-none h-7" />
      </div>
      {platform === "macos" ? null : <WindowControls />}
    </header>
  );
}
