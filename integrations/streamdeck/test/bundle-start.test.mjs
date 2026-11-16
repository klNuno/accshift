// Starts the built bin/plugin.js the way Stream Deck does (`-port`,
// `-pluginUUID`, `-registerEvent`, `-info`) against a fake Stream Deck
// WebSocket server, and waits for it to register and to draw a key. The
// unit tests never load the bundle, so a bundle that dies on start (the
// `Dynamic require of "events"` of an ESM bundle without `require`) only
// shows up here.
//
// Needs `pnpm install` (esbuild and the SDK). Without it the test is skipped
// locally and fails on CI.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const PLUGIN_UUID = "dev.mtsu.accshift";
const DEPS = ["esbuild", "@elgato/streamdeck"].every((dep) =>
  existsSync(join(ROOT, "node_modules", dep)),
);
const skip = !DEPS && !process.env.CI && "run `pnpm install` in integrations/streamdeck first";

/** The smallest WebSocket server Stream Deck's side needs: text frames both ways. */
function fakeStreamDeck() {
  const received = [];
  const waiters = [];
  let socket = null;
  const server = createServer();
  server.on("upgrade", (req, sock) => {
    socket = sock;
    const accept = createHash("sha1")
      .update(`${req.headers["sec-websocket-key"]}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
      .digest("base64");
    sock.write(
      "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
        `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
    );
    let buffer = Buffer.alloc(0);
    sock.on("data", (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      for (;;) {
        const frame = readFrame(buffer);
        if (!frame) break;
        buffer = buffer.subarray(frame.size);
        if (frame.opcode !== 1) continue;
        const message = JSON.parse(frame.payload.toString("utf8"));
        received.push(message);
        for (const w of [...waiters]) {
          if (w.match(message)) {
            waiters.splice(waiters.indexOf(w), 1);
            w.resolve(message);
          }
        }
      }
    });
    sock.on("error", () => {});
  });
  return {
    received,
    listen: () =>
      new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port))),
    send(message) {
      const payload = Buffer.from(JSON.stringify(message), "utf8");
      const head =
        payload.length < 126
          ? Buffer.from([0x81, payload.length])
          : Buffer.from([0x81, 126, payload.length >> 8, payload.length & 0xff]);
      socket.write(Buffer.concat([head, payload]));
    },
    waitFor(match, what) {
      const seen = received.find(match);
      if (seen) return Promise.resolve(seen);
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`no ${what} within 15 s`)), 15_000).unref();
        waiters.push({
          match,
          resolve: (m) => {
            clearTimeout(timer);
            resolve(m);
          },
        });
      });
    },
    close() {
      socket?.destroy();
      server.close();
    },
  };
}

/** One client frame (always masked), or null until it is all there. */
function readFrame(buffer) {
  if (buffer.length < 2) return null;
  const opcode = buffer[0] & 0x0f;
  const masked = (buffer[1] & 0x80) !== 0;
  let length = buffer[1] & 0x7f;
  let offset = 2;
  if (length === 126) {
    if (buffer.length < 4) return null;
    length = buffer.readUInt16BE(2);
    offset = 4;
  } else if (length === 127) {
    if (buffer.length < 10) return null;
    length = Number(buffer.readBigUInt64BE(2));
    offset = 10;
  }
  const mask = masked ? buffer.subarray(offset, offset + 4) : null;
  if (masked) offset += 4;
  if (buffer.length < offset + length) return null;
  const payload = Buffer.from(buffer.subarray(offset, offset + length));
  if (mask) for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4];
  return { opcode, payload, size: offset + length };
}

test("the built bundle starts, registers with Stream Deck and draws a key", { skip, timeout: 60_000 }, async (t) => {
  const { bundleEntry } = await import("../scripts/build.mjs");
  const work = mkdtempSync(join(tmpdir(), "accshift-streamdeck-"));
  const pluginDir = join(work, `${PLUGIN_UUID}.sdPlugin`);
  await bundleEntry(pluginDir);
  copyFileSync(join(ROOT, `${PLUGIN_UUID}.sdPlugin`, "manifest.json"), join(pluginDir, "manifest.json"));

  const deck = fakeStreamDeck();
  const port = await deck.listen();
  const info = {
    application: { font: "", language: "en", platform: "windows", platformVersion: "10", version: "7.1.0" },
    plugin: { uuid: PLUGIN_UUID, version: "0.1.0.0" },
    devicePixelRatio: 1,
    colors: {},
    devices: [{ id: "deck1", name: "Fake deck", size: { columns: 5, rows: 3 }, type: 0 }],
  };
  let stderr = "";
  const child = spawn(
    process.execPath,
    [
      join(pluginDir, "bin", "plugin.js"),
      "-port", String(port),
      "-pluginUUID", PLUGIN_UUID,
      "-registerEvent", "registerPlugin",
      "-info", JSON.stringify(info),
    ],
    // No accshift here: the key below has no platform, so no CLI is run.
    { cwd: pluginDir, env: { ...process.env, ACCSHIFT_CLI: join(work, "none.exe") }, stdio: ["ignore", "ignore", "pipe"] },
  );
  child.stderr.on("data", (d) => {
    stderr += d;
  });
  const exited = new Promise((resolve) => child.on("exit", (code) => resolve(code)));
  t.after(async () => {
    if (child.exitCode === null) child.kill();
    await exited;
    deck.close();
    rmSync(work, { recursive: true, force: true, maxRetries: 5 });
  });
  const died = exited.then((code) => {
    throw new Error(`plugin exited with ${code} before registering: ${stderr}`);
  });

  const register = await Promise.race([
    deck.waitFor((m) => m.event === "registerPlugin", "registration"),
    died,
  ]);
  assert.deepEqual(register, { event: "registerPlugin", uuid: PLUGIN_UUID });

  deck.send({
    event: "willAppear",
    action: "dev.mtsu.accshift.switch",
    context: "key1",
    device: "deck1",
    payload: {
      controller: "Keypad",
      coordinates: { column: 0, row: 0 },
      isInMultiAction: false,
      settings: { accountName: "Alice" },
    },
  });
  const title = await Promise.race([
    deck.waitFor((m) => m.event === "setTitle" && m.context === "key1", "setTitle"),
    died,
  ]);
  assert.equal(title.payload.title, "Alice");
  assert.equal(stderr, "");
});
