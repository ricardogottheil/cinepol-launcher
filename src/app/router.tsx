import {
  createHashHistory,
  createRootRoute,
  createRoute,
  createRouter,
  type RouterHistory,
} from "@tanstack/react-router";
import { AccountsPage } from "../features/accounts/AccountsPage";
import { DesignKitPage } from "../features/design-kit/DesignKitPage";
import { HomePage } from "../features/home/HomePage";
import { InstancesPage } from "../features/instances/InstancesPage";
import { PlaceholderPage, type PlaceholderSection } from "../features/placeholder/PlaceholderPage";
import { AppShell } from "./shell/AppShell";

const rootRoute = createRootRoute({ component: AppShell });

const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: () => <HomePage />,
});

function placeholderRoute(section: PlaceholderSection) {
  return createRoute({
    getParentRoute: () => rootRoute,
    path: `/${section}`,
    component: () => <PlaceholderPage section={section} />,
  });
}

const designRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/design",
  component: DesignKitPage,
});

const routeTree = rootRoute.addChildren([
  homeRoute,
  createRoute({ getParentRoute: () => rootRoute, path: "/instances", component: InstancesPage }),
  placeholderRoute("modpacks"),
  createRoute({ getParentRoute: () => rootRoute, path: "/accounts", component: AccountsPage }),
  placeholderRoute("settings"),
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
