import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/Button";
import { Meter } from "../../components/ui/Meter";
import {
  type ClientErrorCode,
  type RunningOperation,
  runProgressDemo,
  toIpcError,
} from "../../ipc/client";

const STEPS = 20;
const INTERVAL_MS = 150;

type DemoState = "idle" | "running" | "done" | ClientErrorCode;

/** Exercises the Channel progress path and cancellation end to end. */
export function ProgressDemo() {
  const { t } = useTranslation();
  const [state, setState] = useState<DemoState>("idle");
  const [done, setDone] = useState(0);
  const operation = useRef<RunningOperation | null>(null);

  function start() {
    setDone(0);
    setState("running");
    const running = runProgressDemo({
      steps: STEPS,
      intervalMs: INTERVAL_MS,
      onProgress: (event) => setDone(event.done),
    });
    operation.current = running;
    running.done.then(
      () => setState("done"),
      (error: unknown) => setState(toIpcError(error).code),
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <Meter label={t("designKit.ipc.progressLabel")} value={done} max={STEPS} />
      <div className="flex items-center gap-4">
        <Button onClick={start} disabled={state === "running"}>
          {t("designKit.ipc.start")}
        </Button>
        <Button onClick={() => void operation.current?.cancel()} disabled={state !== "running"}>
          {t("designKit.ipc.cancel")}
        </Button>
        <span data-readout role="status" className="readout-sm text-engrave">
          {state === "idle" ? "" : t(`designKit.ipc.state.${state}`)}
        </span>
      </div>
    </div>
  );
}
