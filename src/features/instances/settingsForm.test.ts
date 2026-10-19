import { describe, expect, it } from "vitest";
import type { InstanceSettingsDto } from "../../ipc/client";
import { copyName, toFormValues, toSettings } from "./settingsForm";

const dto: InstanceSettingsDto = {
  name: "Survival",
  defaultAccountId: null,
  java: { kind: "managed", path: null },
  minMemoryMb: 512,
  maxMemoryMb: 4096,
  jvmArgs: ["-XX:+UseZGC"],
  window: null,
  env: { MESA_GL_VERSION_OVERRIDE: "4.6" },
};

describe("settingsForm", () => {
  it("round-trips the stored settings", () => {
    expect(toSettings(toFormValues(dto))).toEqual({ ok: true, settings: dto });
  });

  it("reads one JVM argument and one NAME=value pair per line", () => {
    const result = toSettings({
      ...toFormValues(dto),
      jvmArgs: "  -XX:+UseZGC \n\n-Dfoo=bar baz\n",
      env: "A=1\n  B = x=y \n\n",
      customWindow: true,
      width: "1280",
      height: "720",
    });
    expect(result).toEqual({
      ok: true,
      settings: {
        ...dto,
        jvmArgs: ["-XX:+UseZGC", "-Dfoo=bar baz"],
        env: { A: "1", B: "x=y" },
        window: { width: 1280, height: 720 },
      },
    });
  });

  it("points at the first line that is not NAME=value", () => {
    expect(toSettings({ ...toFormValues(dto), env: "A=1\nnot a pair\n=2" })).toEqual({
      ok: false,
      error: { field: "env", line: 2 },
    });
  });

  it("refuses numbers that are not whole and positive", () => {
    expect(toSettings({ ...toFormValues(dto), maxMemory: "4g" })).toEqual({
      ok: false,
      error: { field: "maxMemory" },
    });
    expect(
      toSettings({ ...toFormValues(dto), customWindow: true, width: "-1", height: "720" }),
    ).toEqual({ ok: false, error: { field: "width" } });
  });

  it("keeps a custom Java path and an empty account as the active one", () => {
    const values = toFormValues({
      ...dto,
      java: { kind: "custom", path: "/opt/jdk/bin/java" },
      defaultAccountId: "a1",
    });
    expect(values.defaultAccountId).toBe("a1");
    expect(toSettings({ ...values, defaultAccountId: "" })).toEqual({
      ok: true,
      settings: { ...dto, java: { kind: "custom", path: "/opt/jdk/bin/java" } },
    });
  });

  it("fits copy names into the 64-character limit", () => {
    const template = (name: string) => `${name} (copy)`;
    expect(copyName("Survival", template)).toBe("Survival (copy)");
    const long = "x".repeat(64);
    const copy = copyName(long, template);
    expect(copy.length).toBe(64);
    expect(copy.endsWith(" (copy)")).toBe(true);
  });

  it("cuts copy names on whole characters, counted like the backend", () => {
    const es = (name: string) => `${name} (copia)`;
    const loneSurrogate = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/;
    for (const name of [`${"A".repeat(55)}😀`, "😀".repeat(64), "\u00f1".repeat(64)]) {
      const copy = copyName(name, es);
      expect(copy).not.toMatch(loneSurrogate);
      expect(Array.from(copy).length).toBeLessThanOrEqual(64);
      expect(copy.endsWith(" (copia)")).toBe(true);
    }
    expect(Array.from(copyName("😀".repeat(64), es)).length).toBe(64);
  });
});
