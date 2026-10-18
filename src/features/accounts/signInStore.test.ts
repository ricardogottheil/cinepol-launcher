import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DeviceCodeDto } from "../../ipc/client";
import { useSignInStore } from "./signInStore";

const client = vi.hoisted(() => ({ loginMicrosoft: vi.fn() }));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const code: DeviceCodeDto = {
  userCode: "ABCD-EFGH",
  verificationUri: "https://www.microsoft.com/link",
  expiresInSecs: 900,
};

beforeEach(() => {
  useSignInStore.setState({ signIn: { kind: "idle", error: null } });
});

describe("useSignInStore", () => {
  it("handles a code that arrives before the operation is returned", async () => {
    const cancel = vi.fn().mockResolvedValue(true);
    client.loginMicrosoft.mockImplementation((onCode: (code: DeviceCodeDto) => void) => {
      onCode(code);
      return { operationId: "op", done: new Promise(() => {}), cancel };
    });

    expect(() => useSignInStore.getState().start(() => {})).not.toThrow();
    const state = useSignInStore.getState().signIn;
    expect(state).toMatchObject({ kind: "waiting", code });
    if (state.kind === "waiting") await state.cancel();
    expect(cancel).toHaveBeenCalledOnce();
  });
});
