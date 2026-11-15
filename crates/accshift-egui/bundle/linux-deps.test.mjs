// node --test crates/accshift-egui/bundle/linux-deps.test.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { currentPackages, resignPackage, signerKey } from "./linux-deps.mjs";

const root = join(fileURLToPath(import.meta.url), "..", "..", "..", "..");

test("only this run's packages of this version are rewritten", () => {
  const dir = mkdtempSync(join(tmpdir(), "accshift-deps-"));
  try {
    const old = Date.now() / 1000 - 3600;
    for (const name of ["accshift_1.0.5_amd64.deb", "accshift_1.0.6_amd64.deb", "accshift_1.0.6_amd64.deb.sig"]) {
      writeFileSync(join(dir, name), "");
    }
    writeFileSync(join(dir, "accshift_1.0.60_amd64.deb"), "");
    utimesSync(join(dir, "accshift_1.0.5_amd64.deb"), old, old);
    const since = Date.now() - 60_000;
    assert.deepEqual(currentPackages(dir, ".deb", "1.0.6", since), [join(dir, "accshift_1.0.6_amd64.deb")]);
    assert.deepEqual(currentPackages(dir, ".deb", "1.0.5", since), []);
    assert.deepEqual(currentPackages(join(dir, "none"), ".rpm", "1.0.6", since), []);
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
