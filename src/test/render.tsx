import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { ReactElement } from "react";

/** A fresh query client that never retries, so failures show at once. */
export function createTestQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
}

/** Renders with a fresh query client that never retries, so failures show at once. */
export function renderWithQuery(ui: ReactElement) {
  const client = createTestQueryClient();
  return { ...render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>), client };
}
