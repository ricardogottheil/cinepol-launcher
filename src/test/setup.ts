import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";
import "../i18n";
import { useToastStore } from "../components/ui/toastStore";

// The default 1s wait for findBy*/waitFor is too short on slow CI runners
// (macOS x64), where a first render can take longer and fail the test.
configure({ asyncUtilTimeout: 5_000 });

afterEach(() => {
  cleanup();
  useToastStore.setState({ toasts: [] });
});
