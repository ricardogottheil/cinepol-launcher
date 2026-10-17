import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { useState } from "react";
import { createQueryClient } from "./queryClient";
import { createAppRouter } from "./router";

export function App({ router: injected }: { router?: ReturnType<typeof createAppRouter> }) {
  const [router] = useState(() => injected ?? createAppRouter());
  const [queryClient] = useState(createQueryClient);
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}
