// Records the README demo (docs/demo.gif) straight from the webview.
//
// Two WebSocket connections to the MCP bridge: one loops native screenshots,
// the other drives the UI. Frames are written to disk with their real
// timestamps, so ffmpeg can rebuild the exact timing afterwards. Capturing the
// webview rather than the screen means a take can never pick up anything else
// on the desktop, and the demo dataset is fake (see src/lib/demo/demoCore.ts),
// so no real account is ever on screen.
//
// Usage, from the repo root:
//
//   1. Start the app in demo mode, with the bridge (bash/git-bash):
//        VITE_DEMO=1 pnpm tauri dev --config src-tauri/tauri.mcp.conf.json --features mcp-bridge
//      PowerShell: $env:VITE_DEMO = "1" first, then the same command.
//   2. Size the window to 940x375 (the framing docs/demo.gif was cut for).
//   3. node scripts/record-demo.mjs          # writes ./frames
//   4. cd frames && ffmpeg -f concat -safe 0 -i frames.txt \
//        -vf "fps=12,scale=900:-2:flags=lanczos,split[a][b];\
//             [a]palettegen=max_colors=160:stats_mode=diff[p];\
//             [b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
//        -loop 0 ../docs/demo.gif

import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const PORT = Number(process.env.BRIDGE_PORT ?? 9223);
const OUT = process.env.OUT_DIR ?? "frames";
const FRAME_INTERVAL_MS = 70;

rmSync(OUT, { recursive: true, force: true });
mkdirSync(OUT, { recursive: true });

function connect() {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://127.0.0.1:${PORT}`);
    const pending = new Map();
    let seq = 0;

    ws.addEventListener("message", (event) => {
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch {
        return;
      }
      const entry = pending.get(msg.id);
      if (!entry) return;
      pending.delete(msg.id);
      entry(msg);
    });
    ws.addEventListener("error", reject);
    ws.addEventListener("open", () =>
      resolve({
        send(command, args) {
          const id = `c${++seq}`;
          return new Promise((res, rej) => {
            const timer = setTimeout(() => {
              pending.delete(id);
              rej(new Error(`timeout: ${command}`));
            }, 15000);
            pending.set(id, (msg) => {
              clearTimeout(timer);
              res(msg);
            });
            ws.send(JSON.stringify({ id, command, args }));
          });
        },
        close: () => ws.close(),
      }),
    );
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Real pointer events: the account cards listen on pointerdown/pointerup for
// drag and drop, so element.click() alone never triggers a switch.
const CLICK_HELPER = `
window.__demoClick = (selector, byText) => {
  const nodes = [...document.querySelectorAll(selector)];
  const el = byText ? nodes.find((n) => (n.innerText || "").includes(byText)) : nodes[0];
  if (!el) return "missing:" + (byText || selector);
  const r = el.getBoundingClientRect();
  const x = r.left + r.width / 2;
  const y = r.top + r.height / 2;
  const opts = { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, pointerId: 1, isPrimary: true, pointerType: "mouse" };
  el.dispatchEvent(new PointerEvent("pointerover", opts));
  el.dispatchEvent(new PointerEvent("pointerenter", opts));
  el.dispatchEvent(new MouseEvent("mouseover", opts));
  el.dispatchEvent(new PointerEvent("pointerdown", opts));
  el.dispatchEvent(new MouseEvent("mousedown", opts));
  el.dispatchEvent(new PointerEvent("pointerup", opts));
  el.dispatchEvent(new MouseEvent("mouseup", opts));
  el.dispatchEvent(new MouseEvent("click", opts));
  return "ok";
};
window.__demoHover = (selector, byText) => {
  const nodes = [...document.querySelectorAll(selector)];
  const el = byText ? nodes.find((n) => (n.innerText || "").includes(byText)) : nodes[0];
  if (!el) return "missing";
  const r = el.getBoundingClientRect();
  const opts = { bubbles: true, cancelable: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, pointerId: 1, isPrimary: true, pointerType: "mouse" };
  el.dispatchEvent(new PointerEvent("pointerover", opts));
  el.dispatchEvent(new PointerEvent("pointerenter", opts));
  el.dispatchEvent(new MouseEvent("mouseover", opts));
  el.dispatchEvent(new MouseEvent("mousemove", opts));
  return "ok";
};
window.__demoType = (selector, text) => {
  const el = document.querySelector(selector);
  if (!el) return "missing";
  el.focus();
  el.value = text;
  el.dispatchEvent(new Event("input", { bubbles: true }));
  return "ok";
};
"ready"
`;

const shots = await connect();
const driver = await connect();

const js = async (script) => {
  const res = await driver.send("execute_js", { script });
  if (!res.success) throw new Error(`execute_js failed: ${JSON.stringify(res).slice(0, 200)}`);
  return res.data;
};

await js(CLICK_HELPER);

let frame = 0;
const timeline = [];
let recording = true;

async function captureLoop() {
  while (recording) {
    const started = Date.now();
    try {
      const res = await shots.send("capture_native_screenshot", { format: "png" });
      if (res.success && typeof res.data === "string") {
        const base64 = res.data.slice(res.data.indexOf(",") + 1);
        const name = `f${String(frame++).padStart(4, "0")}.png`;
        writeFileSync(join(OUT, name), Buffer.from(base64, "base64"));
        timeline.push({ name, at: started });
      }
    } catch (err) {
      console.error("frame failed:", err.message);
    }
    const spent = Date.now() - started;
    if (spent < FRAME_INTERVAL_MS) await sleep(FRAME_INTERVAL_MS - spent);
  }
}

const click = (selector, text) =>
  js(`window.__demoClick(${JSON.stringify(selector)}, ${JSON.stringify(text ?? null)})`);
const hover = (selector, text) =>
  js(`window.__demoHover(${JSON.stringify(selector)}, ${JSON.stringify(text ?? null)})`);
const type = (selector, text) =>
  js(`window.__demoType(${JSON.stringify(selector)}, ${JSON.stringify(text)})`);

async function scenario() {
  await sleep(1200); // let the grid settle on screen

  // 1. Switch account: Nova -> Kestrel. The first click arms the on-card
  // confirmation, the second one runs the switch.
  await hover(".card", "Kestrel");
  await sleep(500);
  await click(".card", "Kestrel");
  await sleep(1100);
  await click(".card", "Kestrel");
  await sleep(2600);

  // 2. Search
  await type("input.search-input", "bram");
  await sleep(1600);
  await type("input.search-input", "");
  await sleep(900);

  // 3. Open the Smurfs folder, then come back
  await click(".card", "Smurfs");
  await sleep(2000);
  await click(".card", "Back");
  await sleep(1500);

  // 4. List view, then back to grid
  await click('button[title="List view"]');
  await sleep(2000);
  await click('button[title="Grid view"]');
  await sleep(1500);
}

const loop = captureLoop();
try {
  await scenario();
} finally {
  recording = false;
  await loop;
}

writeFileSync(join(OUT, "timeline.json"), JSON.stringify(timeline, null, 1));

// ffmpeg concat list with the real per-frame durations.
const lines = [];
for (let i = 0; i < timeline.length; i++) {
  const next = timeline[i + 1];
  const duration = next ? (next.at - timeline[i].at) / 1000 : FRAME_INTERVAL_MS / 1000;
  lines.push(`file '${timeline[i].name}'`, `duration ${duration.toFixed(3)}`);
}
if (timeline.length) lines.push(`file '${timeline[timeline.length - 1].name}'`);
writeFileSync(join(OUT, "frames.txt"), lines.join("\n") + "\n");

const span = timeline.length ? (timeline.at(-1).at - timeline[0].at) / 1000 : 0;
console.log(
  `captured ${timeline.length} frames in ${span.toFixed(1)}s (${(timeline.length / (span || 1)).toFixed(1)} fps)`,
);

shots.close();
driver.close();
