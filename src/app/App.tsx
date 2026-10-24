import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { useState } from "react";
import { createQueryClient } from "./queryClient";
import { createAppRouter } from "./router";

export function App({
  router: injected,
  queryClient: injectedClient,
}: {
  router?: ReturnType<typeof createAppRouter>;
  /** Tests pass a client that never retries. */
  queryClient?: QueryClient;
}) {
  const [router] = useState(() => injected ?? createAppRouter());
  const [queryClient] = useState(() => injectedClient ?? createQueryClient());
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}
