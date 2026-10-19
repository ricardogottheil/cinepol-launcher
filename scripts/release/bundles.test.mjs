import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { collect, EXPECTED_PLATFORMS, manifest, plan } from "./bundles.mjs";

const scratch = mkdtempSync(join(tmpdir(), "cinepol-release-"));
after(() => rmSync(scratch, { recursive: true, force: true }));

function bundleDir(name, files) {
  const dir = join(scratch, name);
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(join(dir, path, ".."), { recursive: true });
    writeFileSync(join(dir, path), content);
  }
  return dir;
}

describe("plan", () => {
  it("names Windows installers and maps their updater platforms", () => {
    const files = [
      "nsis/Cinepol Launcher_0.2.0_x64-setup.exe",
      "nsis/Cinepol Launcher_0.2.0_x64-setup.exe.sig",
      "msi/Cinepol Launcher_0.2.0_x64_en-US.msi",
      "msi/Cinepol Launcher_0.2.0_x64_en-US.msi.sig",
    ];
    assert.deepEqual(plan(files, { os: "windows", arch: "x86_64", version: "0.2.0" }), [
      {
        source: "nsis/Cinepol Launcher_0.2.0_x64-setup.exe",
        name: "cinepol-launcher_0.2.0_windows_x86_64-setup.exe",
        platforms: ["windows-x86_64", "windows-x86_64-nsis"],
      },
      {
        source: "msi/Cinepol Launcher_0.2.0_x64_en-US.msi",
        name: "cinepol-launcher_0.2.0_windows_x86_64.msi",
        platforms: ["windows-x86_64-msi"],
      },
    ]);
  });

  it("serves both Mac architectures from the universal app", () => {
    const files = [
      "macos/Cinepol Launcher.app.tar.gz",
      "macos/Cinepol Launcher.app.tar.gz.sig",
      "dmg/Cinepol Launcher_0.2.0_universal.dmg",
    ];
    assert.deepEqual(plan(files, { os: "macos", arch: "universal", version: "0.2.0" }), [
      {
        source: "macos/Cinepol Launcher.app.tar.gz",
        name: "cinepol-launcher_0.2.0_macos_universal.app.tar.gz",
        platforms: ["darwin-aarch64", "darwin-x86_64", "darwin-aarch64-app", "darwin-x86_64-app"],
      },
      {
        source: "dmg/Cinepol Launcher_0.2.0_universal.dmg",
        name: "cinepol-launcher_0.2.0_macos_universal.dmg",
        platforms: [],
      },
    ]);
  });

  it("maps the Linux AppImage and deb", () => {
    const files = [
      "appimage/Cinepol Launcher_0.2.0_aarch64.AppImage",
      "appimage/Cinepol Launcher_0.2.0_aarch64.AppImage.sig",
      "deb/Cinepol Launcher_0.2.0_arm64.deb",
      "deb/Cinepol Launcher_0.2.0_arm64.deb.sig",
    ];
    const names = plan(files, { os: "linux", arch: "aarch64", version: "0.2.0" }).map((f) => [
      f.name,
      f.platforms,
    ]);
    assert.deepEqual(names, [
      [
        "cinepol-launcher_0.2.0_linux_aarch64.AppImage",
        ["linux-aarch64", "linux-aarch64-appimage"],
      ],
      ["cinepol-launcher_0.2.0_linux_aarch64.deb", ["linux-aarch64-deb"]],
    ]);
  });

  it("refuses an updater bundle without a signature, a stale second bundle or a missing one", () => {
    const options = { os: "linux", arch: "x86_64", version: "0.2.0" };
    assert.throws(
      () => plan(["appimage/a.AppImage", "deb/a.deb", "deb/a.deb.sig"], options),
      /no signature/,
    );
    assert.throws(
      () =>
        plan(
          [
            "appimage/a.AppImage",
            "appimage/a.AppImage.sig",
            "appimage/b.AppImage",
            "deb/a.deb",
            "deb/a.deb.sig",
          ],
          options,
        ),
      /more than one/,
    );
    assert.throws(
      () => plan(["appimage/a.AppImage", "appimage/a.AppImage.sig"], options),
      /no deb/,
    );
    assert.throws(() => plan([], { ...options, os: "plan9" }), /unknown os/);
  });
});

describe("collect and manifest", () => {
  it("copies bundles under their release names and builds latest.json from every fragment", () => {
    const out = join(scratch, "out");
    collect({
      bundleDir: bundleDir("win", {
        "nsis/A_0.2.0_x64-setup.exe": "exe",
        "nsis/A_0.2.0_x64-setup.exe.sig": "sig-nsis",
        "msi/A_0.2.0_x64_en-US.msi": "msi",
        "msi/A_0.2.0_x64_en-US.msi.sig": "sig-msi",
      }),
      outDir: out,
      os: "windows",
      arch: "x86_64",
      version: "0.2.0",
    });
    assert.equal(
      readFileSync(join(out, "cinepol-launcher_0.2.0_windows_x86_64-setup.exe"), "utf8"),
      "exe",
    );

    const base = "https://github.com/o/r/releases/download/v0.2.0";
    const partial = () =>
      manifest({ dir: out, version: "0.2.0", baseUrl: base, notes: "n", pubDate: "d" });
    assert.throws(partial, /missing platforms: windows-aarch64/);

    const result = manifest({
      dir: out,
      version: "0.2.0",
      baseUrl: base,
      notes: "Faster installs",
      pubDate: "2026-10-19T00:00:00Z",
      expected: ["windows-x86_64"],
    });
    assert.deepEqual(result, {
      version: "0.2.0",
      notes: "Faster installs",
      pub_date: "2026-10-19T00:00:00Z",
      platforms: {
        "windows-x86_64": {
          signature: "sig-nsis",
          url: `${base}/cinepol-launcher_0.2.0_windows_x86_64-setup.exe`,
        },
        "windows-x86_64-nsis": {
          signature: "sig-nsis",
          url: `${base}/cinepol-launcher_0.2.0_windows_x86_64-setup.exe`,
        },
        "windows-x86_64-msi": {
          signature: "sig-msi",
          url: `${base}/cinepol-launcher_0.2.0_windows_x86_64.msi`,
        },
      },
    });
    assert.deepEqual(JSON.parse(readFileSync(join(out, "latest.json"), "utf8")), result);
  });

  it("expects every shipped platform by default", () => {
    assert.deepEqual(EXPECTED_PLATFORMS, [
      "windows-x86_64",
      "windows-aarch64",
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-x86_64",
      "linux-aarch64",
    ]);
  });
});

describe("command line", () => {
  it("runs, and fails on errors, also when started through a symlinked path", () => {
    const linked = join(scratch, "linked");
    symlinkSync(dirname(fileURLToPath(import.meta.url)), linked);
    const empty = join(scratch, "empty");
    mkdirSync(empty);
    const run = spawnSync(
      process.execPath,
      [join(linked, "bundles.mjs"), "manifest", empty, "0.2.0", "https://x"],
      {
        encoding: "utf8",
      },
    );
    assert.equal(run.status, 1, run.stdout + run.stderr);
    assert.match(run.stderr, /missing platforms/);
  });
});
