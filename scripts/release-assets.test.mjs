import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { DOWNLOADS, stableName } from "./release-assets.mjs";

// The 2.0.0 release assets, as the build jobs named them.
const BUILT = [
  "Accshift_2.0.0_x64-setup.exe",
  "Accshift_2.0.0_x64-setup.exe.sig",
  "Accshift_2.0.0_aarch64.dmg",
  "Accshift.app.tar.gz",
  "Accshift.app.tar.gz.sig",
  "accshift_2.0.0_amd64.AppImage",
  "accshift_2.0.0_amd64.AppImage.sig",
  "accshift_2.0.0_amd64.deb",
  "accshift_2.0.0_amd64.deb.sig",
  "accshift-2.0.0-1.x86_64.rpm",
  "accshift-2.0.0-1.x86_64.rpm.sig",
  "accshift-cli_2.0.0_x64.exe",
  "accshift-cli_2.0.0_linux_x86_64",
  "accshift-cli_2.0.0_macos_aarch64",
  "accshift-streamdeck_2.0.0.streamDeckPlugin",
];
const script = fileURLToPath(new URL("./release-assets.mjs", import.meta.url));

test("each download gets a name without a version, its signature beside it", () => {
  assert.equal(stableName("Accshift_12.3.45_x64-setup.exe"), "accshift-windows-x64-setup.exe");
  assert.equal(stableName("Accshift_2.0.0_x64-setup.exe.sig"), "accshift-windows-x64-setup.exe.sig");
  assert.equal(stableName("accshift-2.0.0-1.x86_64.rpm.sig"), "accshift-linux-x86_64.rpm.sig");
  for (const kept of ["Accshift.app.tar.gz", "Accshift.app.tar.gz.sig", "latest.json", "SHA256SUMS.txt"]) {
    assert.equal(stableName(kept), kept);
  }
  for (const [name] of DOWNLOADS) assert.doesNotMatch(name, /\d+\.\d+\.\d+/);
});

test("the script renames a release folder and every download is there", () => {
  const dir = mkdtempSync(join(tmpdir(), "accshift-assets-"));
  try {
    for (const file of BUILT) writeFileSync(join(dir, file), file);
    execFileSync(process.execPath, [script, dir], { stdio: "pipe" });
    const files = readdirSync(dir);
    for (const [name] of DOWNLOADS) assert.ok(files.includes(name), name);
    assert.equal(files.length, BUILT.length);
    assert.ok(files.includes("accshift-linux-x86_64.AppImage.sig"));
    assert.ok(files.includes("Accshift.app.tar.gz"));
    assert.ok(!files.some((file) => /2\.0\.0/.test(file)));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a missing download fails the script", () => {
  const dir = mkdtempSync(join(tmpdir(), "accshift-assets-"));
  try {
    for (const file of BUILT.filter((file) => !file.endsWith(".dmg"))) writeFileSync(join(dir, file), file);
    assert.throws(() => execFileSync(process.execPath, [script, dir], { stdio: "pipe" }), /accshift-macos-arm64\.dmg/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
