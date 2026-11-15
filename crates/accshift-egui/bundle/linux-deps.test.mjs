// node --test crates/accshift-egui/bundle/linux-deps.test.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  APPIMAGE_DLOPEN_LIBS,
  appImageFiles,
  clearPackageDirs,
  currentPackages,
  debDepends,
  dropRpmRequires,
  packageKinds,
  resignPackage,
  rpmRequires,
  setDebDepends,
  signerKey,
} from "./linux-deps.mjs";
import {
  BIN,
  header,
  INT32,
  makeDeb,
  makeRpm,
  readControlTar,
  readDeb,
  readRpm,
  STRING,
  STRING_ARRAY,
} from "./linux-deps.fixtures.mjs";

const root = join(fileURLToPath(import.meta.url), "..", "..", "..", "..");

test("only this run's packages of this version are rewritten", () => {
  const dir = mkdtempSync(join(tmpdir(), "accshift-deps-"));
  try {
    const deb = join(dir, "deb");
    mkdirSync(deb);
    mkdirSync(join(dir, "appimage"));
    writeFileSync(join(deb, "accshift_1.0.6_amd64.deb"), "");
    writeFileSync(join(deb, "accshift_1.0.6_amd64.deb.sig"), "");
    assert.deepEqual(packageKinds("appimage, deb,rpm"), ["deb", "rpm"]);

    // A run that builds no deb neither clears nor rewrites the one left there.
    const appimageOnly = packageKinds("appimage");
    assert.deepEqual(appimageOnly, []);
    clearPackageDirs(dir, appimageOnly);
    assert.equal(existsSync(join(deb, "accshift_1.0.6_amd64.deb")), true);
    assert.deepEqual(currentPackages(dir, appimageOnly, "deb", "1.0.6"), []);

    const all = packageKinds("appimage,deb,rpm");
    clearPackageDirs(dir, all);
    assert.equal(existsSync(deb), false);
    assert.equal(existsSync(join(dir, "appimage")), true);

    mkdirSync(deb);
    for (const name of ["accshift_1.0.6_amd64.deb", "accshift_1.0.6_amd64.deb.sig", "accshift_1.0.60_amd64.deb"]) {
      writeFileSync(join(deb, name), "");
    }
    assert.deepEqual(currentPackages(dir, all, "deb", "1.0.6"), [join(deb, "accshift_1.0.6_amd64.deb")]);
    assert.deepEqual(currentPackages(dir, all, "rpm", "1.0.6"), []);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a key file in TAURI_SIGNING_PRIVATE_KEY signs through --private-key-path", () => {
  const dir = mkdtempSync(join(tmpdir(), "accshift-sign-"));
  try {
    const tauri = createRequire(join(root, "package.json")).resolve("@tauri-apps/cli/tauri.js");
    const key = join(dir, "updater.key");
    execFileSync(process.execPath, [tauri, "signer", "generate", "--ci", "-p", "t", "-w", key], { stdio: "ignore" });
    const file = join(dir, "accshift_1.0.6_amd64.deb");
    writeFileSync(file, "payload");
    const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY: key, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "t" };
    const sign = (args, env) =>
      execFileSync(process.execPath, [tauri, "signer", "sign", ...args, "--app-version", "1.0.6", file], {
        env,
        stdio: "ignore",
      });
    // What the signer does with the path itself: the bug.
    assert.throws(() => sign([], env));
    const fixed = signerKey(env);
    assert.deepEqual(fixed.args, ["--private-key-path", key]);
    assert.equal(fixed.env.TAURI_SIGNING_PRIVATE_KEY, undefined);
    sign(fixed.args, fixed.env);

    const content = { TAURI_SIGNING_PRIVATE_KEY: "dW50cnVzdGVk" };
    assert.deepEqual(signerKey(content), { args: [], env: content });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("--no-updater drops a stale .sig without calling the signer", () => {
  const dir = mkdtempSync(join(tmpdir(), "accshift-resign-"));
  try {
    const file = join(dir, "accshift_1.0.6_amd64.deb");
    writeFileSync(file, "payload");
    writeFileSync(`${file}.sig`, "stale");
    const calls = [];
    resignPackage(file, false, (f) => calls.push(f));
    assert.deepEqual(calls, []);
    assert.equal(existsSync(`${file}.sig`), false);
    // Without a .sig either: still no signer.
    resignPackage(file, false, (f) => calls.push(f));
    assert.deepEqual(calls, []);

    // With updater artifacts, the bundler's .sig is replaced by the signer's.
    writeFileSync(`${file}.sig`, "bundler");
    resignPackage(file, true, (f) => {
      calls.push(f);
      writeFileSync(`${f}.sig`, "new");
    });
    assert.deepEqual(calls, [file]);
    assert.throws(() => resignPackage(file, true, () => {}), /no updater signature/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the AppImage carries the libraries winit opens at run time, for the build arch", () => {
  const ldconfig = [
    "1234 libs found in cache `/etc/ld.so.cache'",
    "\tlibxkbcommon.so.0 (libc6,x86-64) => /lib/x86_64-linux-gnu/libxkbcommon.so.0",
    "\tlibxkbcommon.so.0 (libc6) => /lib/i386-linux-gnu/libxkbcommon.so.0",
    "\tlibxkbcommon-x11.so.0 (libc6,x86-64) => /lib/x86_64-linux-gnu/libxkbcommon-x11.so.0",
    "\tlibxkbcommon-x11.so.0 (libc6,AArch64) => /lib/aarch64-linux-gnu/libxkbcommon-x11.so.0",
  ].join("\n");
  assert.deepEqual(appImageFiles(["libxkbcommon.so.0", "libxkbcommon-x11.so.0"], "x86_64-unknown-linux-gnu", ldconfig), {
    "/usr/lib/libxkbcommon.so.0": "/lib/x86_64-linux-gnu/libxkbcommon.so.0",
    "/usr/lib/libxkbcommon-x11.so.0": "/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0",
  });
  assert.throws(
    () => appImageFiles(["libxkbcommon.so.0"], "aarch64-unknown-linux-gnu", ldconfig),
    /libxkbcommon\.so\.0 .*aarch64/,
  );
  for (const name of ["libxkbcommon.so.0", "libxkbcommon-x11.so.0", "libwayland-client.so.0", "libwayland-cursor.so.0"]) {
    assert.ok(APPIMAGE_DLOPEN_LIBS.includes(name), name);
  }
});

// --- the deb and rpm rewrite, on fixtures (linux-deps.fixtures.mjs)

const CONTROL = [
  "Package: accshift",
  "Version: 1.0.6",
  "Architecture: amd64",
  "Depends: libwebkit2gtk-4.1-0, libgtk-3-0, libc6",
  "Description: Account switcher",
  "",
].join("\n");

function withTemp(name, bytes, run) {
  const dir = mkdtempSync(join(tmpdir(), "accshift-pkg-"));
  try {
    const file = join(dir, name);
    writeFileSync(file, bytes);
    run(file);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("setDebDepends rewrites Depends alone and keeps the payload bytes", () => {
  const payload = Buffer.from("data.tar.gz bytes \u0000\u0001 kept as is");
  withTemp("a.deb", makeDeb(CONTROL, payload), (file) => {
    assert.deepEqual(debDepends(file), ["libwebkit2gtk-4.1-0", "libgtk-3-0", "libc6"]);
    const before = readDeb(readFileSync(file));
    setDebDepends(file, ["libc6", "ca-certificates"]);
    const after = readDeb(readFileSync(file));
    assert.deepEqual(debDepends(file), ["libc6", "ca-certificates"]);
    assert.deepEqual(Object.keys(after), ["debian-binary", "control.tar.gz", "data.tar.gz"]);
    assert.ok(after["data.tar.gz"].equals(payload));
    assert.ok(after["debian-binary"].equals(before["debian-binary"]));
    const control = readControlTar(after["control.tar.gz"]);
    assert.equal(control["./control"], CONTROL.replace(/^Depends: .*$/m, "Depends: libc6, ca-certificates"));
    assert.equal(control["./md5sums"], readControlTar(before["control.tar.gz"])["./md5sums"]);
  });
  // No Depends yet: the field goes before the description.
  withTemp("b.deb", makeDeb(CONTROL.replace(/^Depends: .*\n/m, ""), payload), (file) => {
    assert.deepEqual(debDepends(file), []);
    setDebDepends(file, ["libc6"]);
    assert.match(readControlTar(readDeb(readFileSync(file))["control.tar.gz"])["./control"], /Depends: libc6\nDescription:/);
  });
});

const REQUIRES = ["libwebkit2gtk-4.1.so.0()(64bit)", "libc.so.6()(64bit)", "rpmlib(CompressedFileNames)", "libgtk-3.so.0()(64bit)", "ca-certificates"];
const FLAGS = [0x4000, 0x4001, 0x1000008, 0x4002, 0x4003];
const VERSIONS = ["", "", "3.0.4-1", "", ""];

function mainHeader(overrides = {}) {
  return header([
    [100, STRING_ARRAY, ["C"]],
    [1000, STRING, "accshift"],
    [1001, STRING, "1.0.6"],
    [1048, INT32, overrides.flags ?? FLAGS],
    [1049, STRING_ARRAY, REQUIRES],
    [1050, STRING_ARRAY, overrides.versions ?? VERSIONS],
    [1124, STRING, "cpio"],
  ]);
}

const dropWebview = (name) => name.includes("webkit") || name.includes("gtk");

test("dropRpmRequires keeps the require arrays aligned and the digests and payload right", () => {
  const payload = Buffer.from("cpio payload \u0000 bytes");
  withTemp("a.rpm", makeRpm(mainHeader(), payload, [[62, BIN, Buffer.alloc(16, 7)]]), (file) => {
    assert.deepEqual(rpmRequires(file), REQUIRES);
    const before = readRpm(readFileSync(file));
    assert.deepEqual(before.checks, { size: true, sha1: true, sha256: true, md5: true });
    dropRpmRequires(file, dropWebview);
    const after = readRpm(readFileSync(file));
    assert.deepEqual(rpmRequires(file), ["libc.so.6()(64bit)", "rpmlib(CompressedFileNames)", "ca-certificates"]);
    // Each kept name keeps its own flags and version.
    assert.deepEqual(after.main[1049], ["libc.so.6()(64bit)", "rpmlib(CompressedFileNames)", "ca-certificates"]);
    assert.deepEqual(after.main[1048], [0x4001, 0x1000008, 0x4003]);
    assert.deepEqual(after.main[1050], ["", "3.0.4-1", ""]);
    // Other tags, the payload and the signature header's size stay.
    for (const tag of [100, 1000, 1001, 1124]) assert.deepEqual(after.main[tag], before.main[tag]);
    assert.ok(after.payload.equals(payload));
    assert.equal(after.sigBytes.length, before.sigBytes.length);
    assert.ok(after.sig[62].equals(before.sig[62]));
    assert.deepEqual(after.checks, { size: true, sha1: true, sha256: true, md5: true });
  });
});

test("dropRpmRequires leaves the file alone when nothing is dropped", () => {
  const bytes = makeRpm(mainHeader(), Buffer.from("payload"));
  withTemp("a.rpm", bytes, (file) => {
    dropRpmRequires(file, () => false);
    assert.ok(readFileSync(file).equals(bytes));
  });
});

test("dropRpmRequires refuses what it cannot rewrite, leaving the file alone", () => {
  // An RSA signature (268) covers the header and cannot be computed again.
  const signed = makeRpm(mainHeader(), Buffer.from("payload"), [[268, BIN, Buffer.alloc(32, 1)]]);
  withTemp("signed.rpm", signed, (file) => {
    assert.throws(() => dropRpmRequires(file, dropWebview), /signature tag 268 covers the header/);
    assert.ok(readFileSync(file).equals(signed));
  });
  // A version array that does not match the names.
  const skewed = makeRpm(mainHeader({ versions: ["", ""] }), Buffer.from("payload"));
  withTemp("skewed.rpm", skewed, (file) => {
    assert.throws(() => dropRpmRequires(file, dropWebview), /tag 1050 has 2 values, not 5/);
    assert.ok(readFileSync(file).equals(skewed));
  });
});
