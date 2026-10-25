import { Outlet } from "@tanstack/react-router";
import { useMemo } from "react";
import { Toaster } from "../../components/ui/Toaster";
import { detectPlatform } from "../../ipc/window";
import { Sidebar } from "./Sidebar";
import { TitleBar } from "./TitleBar";

/** Window frame: title bar on top, section rail on the left, the page scrolls on its own. */
export function AppShell() {
  const platform = useMemo(() => detectPlatform(), []);
  return (
    <div className="flex h-full flex-col bg-chassis text-silk">
      <TitleBar platform={platform} />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <main className="min-w-0 flex-1 overflow-y-auto">
          <Outlet />
        </main>
      </div>
      <Toaster />
    </div>
  );
}
