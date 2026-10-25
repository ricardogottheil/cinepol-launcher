/** One block of release notes: a paragraph or a bulleted list. */
export type NotesBlock = { kind: "paragraph"; text: string } | { kind: "list"; items: string[] };

/** A line that only repeats the release name the page already shows. */
const RELEASE_NAME = /^cinepol launcher v?\d+(\.\d+)*\S*$/i;

/**
 * Release notes as the tag message writes them (docs/release.md): English,
 * then `---`, then Spanish. Picks the block for `language` (English when that
 * language has none, or only the release name) and reads `- ` lines as list items. Text stays text; no
 * markup is interpreted.
 */
export function parseReleaseNotes(notes: string, language: string): NotesBlock[] {
  // Only the first separator divides the languages; later ones are rules.
  const separator = /^\s*---\s*$/m.exec(notes);
  const english = parseBlocks(separator ? notes.slice(0, separator.index) : notes);
  if (!separator || !language.toLowerCase().startsWith("es")) return english;
  const spanish = parseBlocks(notes.slice(separator.index + separator[0].length));
  return spanish.length > 0 ? spanish : english;
}

function parseBlocks(section: string): NotesBlock[] {
  const blocks: NotesBlock[] = [];
  for (const raw of section.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "" || line === "---" || RELEASE_NAME.test(line)) continue;
    const item = /^[-*]\s+(.*)$/.exec(line)?.[1];
    const last = blocks.at(-1);
    if (item === undefined) blocks.push({ kind: "paragraph", text: line });
    else if (last?.kind === "list") last.items.push(item);
    else blocks.push({ kind: "list", items: [item] });
  }
  return blocks;
}
