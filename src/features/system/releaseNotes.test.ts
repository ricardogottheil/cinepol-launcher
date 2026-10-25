import { describe, expect, it } from "vitest";
import { parseReleaseNotes } from "./releaseNotes";

const notes = `Cinepol Launcher 0.3.0

- Servers on Home.
- Toasts.

---

- Servidores en Inicio.
- Avisos.`;

describe("parseReleaseNotes", () => {
  it("reads the block of the player's language as a list", () => {
    expect(parseReleaseNotes(notes, "es")).toEqual([
      { kind: "list", items: ["Servidores en Inicio.", "Avisos."] },
    ]);
  });

  it("uses English for other languages and drops the repeated release name", () => {
    expect(parseReleaseNotes(notes, "en")).toEqual([
      { kind: "list", items: ["Servers on Home.", "Toasts."] },
    ]);
    expect(parseReleaseNotes(notes, "fr")).toEqual(parseReleaseNotes(notes, "en"));
  });

  it("falls back to English when there is no Spanish block", () => {
    expect(parseReleaseNotes("Faster installs", "es")).toEqual([
      { kind: "paragraph", text: "Faster installs" },
    ]);
  });

  it("keeps paragraphs and lists in order and never reads markup", () => {
    expect(parseReleaseNotes("Intro\n- one\n* two\nOutro <b>x</b>", "en")).toEqual([
      { kind: "paragraph", text: "Intro" },
      { kind: "list", items: ["one", "two"] },
      { kind: "paragraph", text: "Outro <b>x</b>" },
    ]);
  });
});
