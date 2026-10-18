import type { InstanceSettingsDto, JavaKindDto } from "../../ipc/client";

/** Instance names are at most this many characters (the backend checks it too). */
export const MAX_INSTANCE_NAME = 64;

/** The settings form as the player types it: numbers and lists are text. */
export interface SettingsFormValues {
  name: string;
  /** `""` launches with the active account. */
  defaultAccountId: string;
  javaKind: JavaKindDto;
  javaPath: string | null;
  minMemory: string;
  maxMemory: string;
  /** One JVM argument per line. */
  jvmArgs: string;
  customWindow: boolean;
  width: string;
  height: string;
  /** One `NAME=value` pair per line. */
  env: string;
}

export type SettingsField = "minMemory" | "maxMemory" | "width" | "height" | "env";

export type SettingsFormResult =
  | { ok: true; settings: InstanceSettingsDto }
  | { ok: false; error: { field: SettingsField; line?: number } };

export function toFormValues(settings: InstanceSettingsDto): SettingsFormValues {
  return {
    name: settings.name,
    defaultAccountId: settings.defaultAccountId ?? "",
    javaKind: settings.java.kind,
    javaPath: settings.java.path,
    minMemory: String(settings.minMemoryMb),
    maxMemory: String(settings.maxMemoryMb),
    jvmArgs: settings.jvmArgs.join("\n"),
    customWindow: settings.window !== null,
    width: settings.window ? String(settings.window.width) : "",
    height: settings.window ? String(settings.window.height) : "",
    env: Object.entries(settings.env)
      .map(([name, value]) => `${name}=${value}`)
      .join("\n"),
  };
}

function wholeNumber(raw: string): number | null {
  const trimmed = raw.trim();
  return /^\d+$/.test(trimmed) ? Number(trimmed) : null;
}

/** Non-blank lines, trimmed, with their 1-based line numbers. */
function lines(raw: string): Array<[number, string]> {
  return raw
    .split("\n")
    .map((line, index): [number, string] => [index + 1, line.trim()])
    .filter(([, line]) => line !== "");
}

/**
 * Turns the form into the backend shape. Only the shape is checked here;
 * the backend validates the values themselves (ranges, reserved arguments).
 */
export function toSettings(values: SettingsFormValues): SettingsFormResult {
  const numbers: Partial<Record<SettingsField, number>> = {};
  const required: SettingsField[] = values.customWindow
    ? ["minMemory", "maxMemory", "width", "height"]
    : ["minMemory", "maxMemory"];
  for (const field of required) {
    const value = wholeNumber(values[field as Exclude<SettingsField, "env">]);
    if (value === null) return { ok: false, error: { field } };
    numbers[field] = value;
  }
  const env: Record<string, string> = {};
  for (const [line, text] of lines(values.env)) {
    const separator = text.indexOf("=");
    const name = separator > 0 ? text.slice(0, separator).trim() : "";
    if (name === "") return { ok: false, error: { field: "env", line } };
    env[name] = text.slice(separator + 1).trim();
  }
  return {
    ok: true,
    settings: {
      name: values.name,
      defaultAccountId: values.defaultAccountId === "" ? null : values.defaultAccountId,
      java: {
        kind: values.javaKind,
        path: values.javaKind === "custom" ? values.javaPath : null,
      },
      minMemoryMb: numbers.minMemory ?? 0,
      maxMemoryMb: numbers.maxMemory ?? 0,
      jvmArgs: lines(values.jvmArgs).map(([, text]) => text),
      window: values.customWindow
        ? { width: numbers.width ?? 0, height: numbers.height ?? 0 }
        : null,
      env,
    },
  };
}

/** A copy's name from a localized template, shortening the original to fit. */
export function copyName(name: string, template: (name: string) => string): string {
  const overflow = template(name).length - MAX_INSTANCE_NAME;
  if (overflow <= 0) return template(name);
  return template(name.slice(0, Math.max(1, name.length - overflow)).trimEnd());
}
