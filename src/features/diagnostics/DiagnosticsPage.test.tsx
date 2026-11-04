import { createMemoryHistory } from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../../app/App";
import { createAppRouter } from "../../app/router";
import enCommon from "../../i18n/locales/en/common.json";
import esCommon from "../../i18n/locales/es/common.json";
import type { DiagnosticDto } from "../../ipc/client";
import { useDiagnostics } from "./diagnosticsStore";
import { cinepolDiagnostic } from "./fixtures";

const client = vi.hoisted(() => ({
  diagnoseServer: vi.fn(),
  shareDiagnosticReport: vi.fn(),
  listSavedServers: vi.fn(async () => []),
  listAccounts: vi.fn(async () => ({ accounts: [], activeId: null })),
  listInstances: vi.fn(async () => []),
  getAppInfo: vi.fn(async () => ({ name: "Cinepol Launcher", version: "0.5.0" })),
  checkAppUpdate: vi.fn(async () => ({ kind: "upToDate" })),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const labels = esCommon.diagnostics;

/** A diagnosis the test drives: `push` sends an update, `finish` ends it. */
function controlledRun() {
  let push: (diagnostic: DiagnosticDto) => void = () => {};
  let finish: (diagnostic: DiagnosticDto) => void = () => {};
  const cancel = vi.fn(async () => true);
  client.diagnoseServer.mockImplementation(
    (_address: string, _secs: number, onUpdate: (d: DiagnosticDto) => void) => {
      push = onUpdate;
      return {
        operationId: "op",
        cancel,
        done: new Promise<DiagnosticDto>((resolve) => {
          finish = resolve;
        }),
      };
    },
  );
  return {
    push: (diagnostic: DiagnosticDto) => act(() => push(diagnostic)),
    finish: (diagnostic: DiagnosticDto) => act(async () => finish(diagnostic)),
    cancel,
  };
}

function renderAt(path: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  return render(<App router={router} />);
}

const startButton = () => screen.findByRole("button", { name: new RegExp(labels.start, "i") });

describe("DiagnosticsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useDiagnostics.setState({
      address: "",
      durationSecs: 300,
      diagnostic: null,
      startedAt: null,
      running: false,
      error: null,
      showPrivate: false,
      shared: null,
      sharing: false,
      shareError: null,
      cancel: null,
    });
  });

  it("tests the server from the link for five minutes by default", async () => {
    const run = controlledRun();
    renderAt("/diagnostics?server=play.example.net");
    expect(await screen.findByRole("heading", { name: labels.title })).toBeInTheDocument();
    expect(await screen.findByDisplayValue("play.example.net")).toBeInTheDocument();
    fireEvent.click(await startButton());

    expect(client.diagnoseServer).toHaveBeenCalledWith(
      "play.example.net",
      300,
      expect.any(Function),
    );
    run.push(
      cinepolDiagnostic({ state: "running", elapsedSecs: 65, verdict: { kind: "measuring" } }),
    );
    expect(screen.getByText(labels.progress.replace("{{left}}", "3:55"))).toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.stop })).toBeInTheDocument();

    await run.finish(cinepolDiagnostic());
    expect(await screen.findByText(labels.verdict.healthy)).toBeInTheDocument();
    expect(screen.getByText("49.3 ms")).toBeInTheDocument();
  });

  it("stops early on request", async () => {
    const run = controlledRun();
    renderAt("/diagnostics?server=play.example.net");
    fireEvent.click(await startButton());
    fireEvent.click(await screen.findByRole("button", { name: labels.stop }));
    expect(run.cancel).toHaveBeenCalled();
  });

  it("explains where a problem starts and what to do", async () => {
    const run = controlledRun();
    renderAt("/diagnostics?server=play.example.net");
    fireEvent.click(await startButton());
    await run.finish(
      cinepolDiagnostic({ verdict: { kind: "issue", issue: "loss", zone: "provider" } }),
    );
    expect(await screen.findByText(labels.verdict.where.provider)).toBeInTheDocument();
    expect(screen.getByText(labels.verdict.issue.loss)).toBeInTheDocument();
    expect(screen.getByText(labels.verdict.advice.provider)).toBeInTheDocument();
  });

  it("hides the router and provider in the hop table until asked", async () => {
    const run = controlledRun();
    renderAt("/diagnostics?server=play.example.net");
    fireEvent.click(await startButton());
    await run.finish(cinepolDiagnostic());
    const table = await screen.findByRole("table");
    expect(within(table).queryByText("192.168.68.1")).toBeNull();
    expect(within(table).getAllByText(labels.hops.hidden.home)).toHaveLength(2);
    expect(within(table).getByText("103.195.101.252")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: labels.hops.showPrivate }));
    expect(within(table).getByText("192.168.68.1")).toBeInTheDocument();
  });

  it("creates a link with the private hops hidden", async () => {
    const run = controlledRun();
    client.shareDiagnosticReport.mockResolvedValue({
      url: "https://pastes.dev/aB3dE9fGh2",
      expiresAt: null,
    });
    renderAt("/diagnostics?server=play.example.net");
    fireEvent.click(await startButton());
    await run.finish(cinepolDiagnostic());
    fireEvent.click(await screen.findByRole("button", { name: labels.share.createLink }));
    expect(
      await screen.findByRole("link", { name: "https://pastes.dev/aB3dE9fGh2" }),
    ).toBeInTheDocument();
    const [report] = client.shareDiagnosticReport.mock.calls[0] as [string];
    expect(report).toContain(enCommon.diagnostics.report.title);
    expect(report).not.toContain("192.168.68.1");
    expect(report).toContain("Client: Cinepol Launcher 0.5.0");
    expect(screen.getByText(new RegExp(labels.share.expires))).toBeInTheDocument();
  });

  it("shows why a report could not be shared", async () => {
    const run = controlledRun();
    const { IpcError } = await import("../../ipc/client");
    client.shareDiagnosticReport.mockRejectedValue(new IpcError("report_rejected", "refused"));
    renderAt("/diagnostics?server=play.example.net");
    fireEvent.click(await startButton());
    await run.finish(cinepolDiagnostic());
    fireEvent.click(await screen.findByRole("button", { name: labels.share.createLink }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(esCommon.errors.report_rejected),
    );
  });
});
