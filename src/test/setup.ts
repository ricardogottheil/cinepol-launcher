import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";
import "../i18n";
import { useToastStore } from "../components/ui/toastStore";

afterEach(() => {
  cleanup();
  useToastStore.setState({ toasts: [] });
});
