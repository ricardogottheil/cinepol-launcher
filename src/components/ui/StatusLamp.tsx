export type ServerStatus = "checking" | "online" | "offline" | "unavailable";

/**
 * The live dot: one of the two places `signal` green may appear. It only
 * shows (and breathes) while the server actually answered.
 */
export function StatusLamp({ status }: { status: ServerStatus }) {
  if (status !== "online") return null;
  return <span data-status-dot aria-hidden="true" className="size-2 rounded-full bg-live" />;
}
