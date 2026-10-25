/** One block of release notes: a paragraph or a bulleted list. */
export type NotesBlock = { kind: "paragraph"; text: string } | { kind: "list"; items: string[] };

/** A line that only repeats the release name the page already shows. */
const RELEASE_NAME = /^cinepol launcher v?\d+(\.\d+)*\S*$/i;

/**
 * Release notes as the tag message writes them (docs/release.md): English,
 * then `---`, then Spanish. Picks the block for `language` (English when that
 * language has none) and reads `- ` lines as list items. Text stays text; no
 * markup is interpreted.
 */
export function parseReleaseNotes(notes: string, language: string): NotesBlock[] {
  const sections = notes.split(/^\s*---\s*$/m).map((section) => section.trim());
  const spanish = language.toLowerCase().startsWith("es");
  const chosen = (spanish ? sections[1] : undefined) || sections[0] || "";

  const blocks: NotesBlock[] = [];
  for (const raw of chosen.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "" || RELEASE_NAME.test(line)) continue;
    const item = /^[-*]\s+(.*)$/.exec(line)?.[1];
    const last = blocks.at(-1);
    if (item === undefined) blocks.push({ kind: "paragraph", text: line });
    else if (last?.kind === "list") last.items.push(item);
    else blocks.push({ kind: "list", items: [item] });
  }
  return blocks;
}
