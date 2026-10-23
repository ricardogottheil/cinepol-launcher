import {
  createHashHistory,
  createRootRoute,
  createRoute,
  createRouter,
  type RouterHistory,
} from "@tanstack/react-router";
import { AddProfilePage } from "../features/accounts/AddProfilePage";
import { ProfilePage } from "../features/accounts/ProfilePage";
import { DesignKitPage } from "../features/design-kit/DesignKitPage";
import { HomePage } from "../features/home/HomePage";
import { InstancesPage } from "../features/instances/InstancesPage";
import { ModpackDetailPage } from "../features/modpacks/ModpackDetailPage";
import { ModpacksPage } from "../features/modpacks/ModpacksPage";
import { SettingsPage } from "../features/settings/SettingsPage";
import { AppShell } from "./shell/AppShell";

const rootRoute = createRootRoute({ component: AppShell });

const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: () => <HomePage />,
});

const designRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/design",
  component: DesignKitPage,
});

const routeTree = rootRoute.addChildren([
  homeRoute,
  createRoute({ getParentRoute: () => rootRoute, path: "/instances", component: InstancesPage }),
  createRoute({ getParentRoute: () => rootRoute, path: "/modpacks", component: ModpacksPage }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: "/modpacks/$source/$project",
    component: ModpackDetailPage,
  }),
  createRoute({ getParentRoute: () => rootRoute, path: "/profile", component: ProfilePage }),
  createRoute({ getParentRoute: () => rootRoute, path: "/profile/new", component: AddProfilePage }),
  createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: SettingsPage }),
  ...(import.meta.env.DEV ? [designRoute] : []),
]);

/**
 * Hash history: the packaged app serves a single index.html, so a reload on a
 * nested path must not ask the asset server for a file that does not exist.
 */
export function createAppRouter(history: RouterHistory = createHashHistory()) {
  return createRouter({ routeTree, history });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
