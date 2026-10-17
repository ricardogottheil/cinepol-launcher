#!/usr/bin/env node
// English-only guard: fails when tracked or new files contain characters that
// signal non-English prose (Spanish accents and punctuation). Translation files
// under src/i18n/locales/<lang>/ (except `en`) are exempt. A line that must
// quote a proper noun can opt out with the marker `english-check: allow`.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const SUSPICIOUS =
  /[\u00bf\u00a1\u00f1\u00d1\u00e1\u00e9\u00ed\u00f3\u00fa\u00c1\u00c9\u00cd\u00d3\u00da\u00fc\u00dc]/u;
const ALLOW_MARKER = "english-check: allow";
const EXEMPT = [
  /^src\/i18n\/locales\/(?!en\/)[^/]+\//,
  /^pnpm-lock\.yaml$/,
  // Real-world data captured from external services (server MOTDs, APIs).
  /^crates\/[^/]+\/tests\/fixtures\//,
];
const BINARY = /\.(png|jpe?g|webp|gif|ico|icns|woff2?|ttf|otf|zip|jar|dat|mrpack)$/i;

const files = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard"], {
  encoding: "utf8",
})
  .split("\n")
  .filter(Boolean)
  .filter((file) => !BINARY.test(file) && !EXEMPT.some((pattern) => pattern.test(file)));

const findings = [];
for (const file of files) {
  let content;
  try {
    content = readFileSync(file, "utf8");
  } catch {
    continue;
  }
  content.split("\n").forEach((line, index) => {
    if (SUSPICIOUS.test(line) && !line.includes(ALLOW_MARKER)) {
      findings.push(`${file}:${index + 1}: ${line.trim()}`);
    }
  });
}

if (findings.length > 0) {
  console.error("Non-English text found (move it to src/i18n/locales/<lang>/):");
  for (const finding of findings) console.error(`  ${finding}`);
  process.exit(1);
}
console.log(`English-only check passed (${files.length} files).`);
