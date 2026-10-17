import { RouterProvider } from "@tanstack/react-router";
import { useState } from "react";
import { createAppRouter } from "./router";

export function App({ router: injected }: { router?: ReturnType<typeof createAppRouter> }) {
  const [router] = useState(() => injected ?? createAppRouter());
  return <RouterProvider router={router} />;
}
