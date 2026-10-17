import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Window controls for the custom title bar. Components use these helpers
 * instead of the Tauri API so they also render in a plain browser (tests,
 * design previews), where every call is a no-op.
 */
export type DesktopPlatform = "macos" | "windows" | "linux";

export function detectPlatform(userAgent: string = navigator.userAgent): DesktopPlatform {
  if (/Mac OS X|Macintosh/i.test(userAgent)) return "macos";
  if (/Windows/i.test(userAgent)) return "windows";
  return "linux";
}

export async function minimizeWindow(): Promise<void> {
  if (isTauri()) await getCurrentWindow().minimize();
}

export async function toggleMaximizeWindow(): Promise<void> {
  if (isTauri()) await getCurrentWindow().toggleMaximize();
}

export async function closeWindow(): Promise<void> {
  if (isTauri()) await getCurrentWindow().close();
}
