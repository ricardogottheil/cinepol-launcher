// Release helpers for .github/workflows/release.yml.
//
//   node scripts/release/bundles.mjs collect <bundleDir> <outDir> <os> <arch> <version>
//     Copies the bundles Tauri produced under fixed release names and writes
//     updater-<os>-<arch>.json with each updater platform's file and signature.
//   node scripts/release/bundles.mjs manifest <dir> <version> <baseUrl> [notesFile]
//     Merges every fragment in <dir> into latest.json for tauri-plugin-updater,
//     refusing a release that misses a shipped platform.

import {
  copyFileSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

/** Every platform a release must serve, in the order of the build matrix. */
export const EXPECTED_PLATFORMS = [
  "windows-x86_64",
  "windows-aarch64",
  "darwin-aarch64",
  "darwin-x86_64",
  "linux-x86_64",
  "linux-aarch64",
];

/**
 * What each OS ships: the bundle folder and suffix Tauri writes, the release
 * file suffix, and the updater platforms that file serves (none for
 * download-only bundles, which need no signature).
 */
const KINDS = {
  windows: [
    {
      dir: "nsis",
      suffix: ".exe",
      as: "-setup.exe",
      platforms: (a) => [`windows-${a}`, `windows-${a}-nsis`],
    },
    {
      dir: "msi",
      suffix: ".msi",
      as: ".msi",
      platforms: (a) => [`windows-${a}-msi`],
      optional: true,
    },
  ],
  macos: [
    {
      dir: "macos",
      suffix: ".app.tar.gz",
      as: ".app.tar.gz",
      platforms: () => [
        "darwin-aarch64",
        "darwin-x86_64",
        "darwin-aarch64-app",
        "darwin-x86_64-app",
      ],
    },
    { dir: "dmg", suffix: ".dmg", as: ".dmg", platforms: () => [] },
  ],
  linux: [
    {
      dir: "appimage",
      suffix: ".AppImage",
      as: ".AppImage",
      platforms: (a) => [`linux-${a}`, `linux-${a}-appimage`],
    },
    { dir: "deb", suffix: ".deb", as: ".deb", platforms: (a) => [`linux-${a}-deb`] },
  ],
};

/**
 * The release files among `files` (paths relative to the bundle folder):
 * exactly one bundle per kind, each updater bundle with its `.sig` next to it.
 */
export function plan(files, { os, arch, version }) {
  const kinds = KINDS[os];
  if (!kinds) throw new Error(`unknown os ${os}`);
  const set = new Set(files);
  const result = [];
  for (const kind of kinds) {
    const matches = files.filter(
      (file) => file.startsWith(`${kind.dir}/`) && file.endsWith(kind.suffix),
    );
    if (matches.length === 0) {
      if (kind.optional) continue;
      throw new Error(`no ${kind.dir} bundle for ${os} ${arch}`);
    }
    if (matches.length > 1) {
      throw new Error(`more than one ${kind.dir} bundle: ${matches.join(", ")} (stale build?)`);
    }
    const [source] = matches;
    const platforms = kind.platforms(arch);
    if (platforms.length > 0 && !set.has(`${source}.sig`)) {
      throw new Error(`${source} has no signature (is TAURI_SIGNING_PRIVATE_KEY set?)`);
    }
    result.push({ source, name: `cinepol-launcher_${version}_${os}_${arch}${kind.as}`, platforms });
  }
  return result;
}

function listFiles(dir) {
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => relative(dir, join(entry.parentPath, entry.name)).split(sep).join("/"));
}

/** Copies the planned bundles into `outDir` and writes this target's fragment. */
export function collect({ bundleDir, outDir, os, arch, version }) {
  const files = plan(listFiles(bundleDir), { os, arch, version });
  mkdirSync(outDir, { recursive: true });
  const platforms = {};
  for (const file of files) {
    copyFileSync(join(bundleDir, file.source), join(outDir, file.name));
    if (file.platforms.length === 0) continue;
    const signature = readFileSync(join(bundleDir, `${file.source}.sig`), "utf8").trim();
    for (const platform of file.platforms) platforms[platform] = { name: file.name, signature };
  }
  writeFileSync(join(outDir, `updater-${os}-${arch}.json`), JSON.stringify({ platforms }, null, 2));
  return files;
}

/** Merges every fragment in `dir` into `latest.json`; refuses gaps and clashes. */
export function manifest({ dir, version, baseUrl, notes, pubDate, expected = EXPECTED_PLATFORMS }) {
  const platforms = {};
  const fragments = readdirSync(dir)
    .filter((name) => /^updater-.+\.json$/.test(name))
    .sort();
  for (const fragment of fragments) {
    const parsed = JSON.parse(readFileSync(join(dir, fragment), "utf8"));
    for (const [platform, { name, signature }] of Object.entries(parsed.platforms)) {
      if (platforms[platform]) throw new Error(`${platform} is served by two bundles`);
      platforms[platform] = { signature, url: `${baseUrl}/${encodeURIComponent(name)}` };
    }
  }
  const missing = expected.filter((platform) => !platforms[platform]);
  if (missing.length > 0) throw new Error(`missing platforms: ${missing.join(", ")}`);
  const result = { version, notes, pub_date: pubDate, platforms };
  writeFileSync(join(dir, "latest.json"), `${JSON.stringify(result, null, 2)}\n`);
  return result;
}

function main([command, ...args]) {
  if (command === "collect" && args.length === 5) {
    const [bundleDir, outDir, os, arch, version] = args;
    for (const file of collect({ bundleDir, outDir, os, arch, version })) console.log(file.name);
    return;
  }
  if (command === "manifest" && (args.length === 3 || args.length === 4)) {
    const [dir, version, baseUrl, notesFile] = args;
    const notes = notesFile ? readFileSync(notesFile, "utf8").trim() : "";
    const result = manifest({ dir, version, baseUrl, notes, pubDate: new Date().toISOString() });
    console.log(`latest.json: ${Object.keys(result.platforms).length} platforms`);
    return;
  }
  console.error("usage: bundles.mjs collect <bundleDir> <outDir> <os> <arch> <version>");
  console.error("       bundles.mjs manifest <dir> <version> <baseUrl> [notesFile]");
  process.exit(2);
}

/** Whether this file is the script node was started with (symlinked paths included). */
function isEntryPoint() {
  const started = process.argv[1];
  if (!started) return false;
  try {
    return realpathSync(started) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
}

if (isEntryPoint()) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
