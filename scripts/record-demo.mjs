// Records the README demos (docs/demo-*.gif) straight from the webview, one
// shot at a time.
//
// Two WebSocket connections to the MCP bridge: one loops native screenshots,
// the other drives the UI. Frames are written with their real timestamps, so
// ffmpeg can rebuild the exact timing afterwards. Nothing here touches the real
// mouse or the window focus, and the frames come from the webview rather than
// the screen, so a take can never pick up anything else on the desktop. The
// pointer visible in the recording is a fake one injected into the page.
//
// The dataset is fake too (src/lib/demo/demoCore.ts), so no real account is
// ever on screen and no click can reach the backend.
//
// Usage, from the repo root:
//
//   1. Start the app in demo mode, with the bridge (bash/git-bash):
//        VITE_DEMO=1 pnpm tauri dev --config src-tauri/tauri.mcp.conf.json --features mcp-bridge
//      PowerShell: $env:VITE_DEMO = "1" first, then the same command.
//   2. Size the window to 940x520 (the framing the clips are cut for).
//   3. node scripts/record-demo.mjs <shot>     # one of the SHOTS keys, or "all"
//   4. node scripts/record-demo.mjs --render   # cut the shots into docs/demo-*.gif
//
// Requires ffmpeg on PATH for --render.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const PORT = Number(process.env.BRIDGE_PORT ?? 9223);
const ROOT = process.env.OUT_DIR ?? "frames";
const FRAME_INTERVAL_MS = 70;
const FFMPEG = process.env.FFMPEG ?? "ffmpeg";

// ---------------------------------------------------------------- bridge glue

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

// ------------------------------------------------------------- page-side helpers

// Injected once per page load. Holds the fake pointer plus the event plumbing:
// the account cards listen on pointer events for drag and drop, so
// element.click() alone never triggers a switch.
const HELPERS = `
(() => {
  const CURSOR_ID = "__demo_cursor";
  document.getElementById(CURSOR_ID)?.remove();

  const cursor = document.createElement("div");
  cursor.id = CURSOR_ID;
  cursor.style.cssText = [
    "position:fixed", "left:0", "top:0", "width:22px", "height:22px",
    "z-index:2147483647", "pointer-events:none", "opacity:0",
    "transform:translate(-2px,-2px)", "will-change:transform",
    "filter:drop-shadow(0 2px 3px rgba(0,0,0,.55))",
  ].join(";");
  cursor.innerHTML =
    '<svg viewBox="0 0 24 24" width="22" height="22">' +
    '<path d="M5 2.5 L5 19 L9.2 15.1 L11.8 21 L14.6 19.7 L12 14 L18 13.8 Z" fill="#fff" stroke="#111" stroke-width="1.1" stroke-linejoin="round"/>' +
    "</svg>";

  const ring = document.createElement("div");
  ring.style.cssText = [
    "position:fixed", "left:0", "top:0", "width:30px", "height:30px",
    "margin:-15px 0 0 -15px", "border-radius:50%", "border:2px solid rgba(255,255,255,.85)",
    "z-index:2147483646", "pointer-events:none", "opacity:0", "transform:scale(.4)",
  ].join(";");

  document.body.append(cursor, ring);

  const state = { x: window.innerWidth / 2, y: window.innerHeight * 0.75 };

  const place = () => {
    cursor.style.transform = "translate(" + state.x + "px," + state.y + "px)";
    ring.style.transform = "translate(" + state.x + "px," + state.y + "px) scale(.4)";
  };
  place();

  window.__demo = {
    cursorShow(on) {
      cursor.style.transition = "opacity .25s ease";
      cursor.style.opacity = on ? "1" : "0";
    },
    // Eased travel so the pointer reads as a hand, not a teleport.
    moveTo(x, y, ms) {
      return new Promise((resolve) => {
        const fromX = state.x;
        const fromY = state.y;
        const started = performance.now();
        const step = (now) => {
          const t = Math.min(1, (now - started) / ms);
          const e = t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
          state.x = fromX + (x - fromX) * e;
          state.y = fromY + (y - fromY) * e;
          place();
          if (t < 1) requestAnimationFrame(step);
          else resolve();
        };
        requestAnimationFrame(step);
      });
    },
    clickFx() {
      ring.style.transition = "none";
      ring.style.opacity = "0.9";
      ring.style.transform = "translate(" + state.x + "px," + state.y + "px) scale(.4)";
      requestAnimationFrame(() => {
        ring.style.transition = "opacity .45s ease, transform .45s ease";
        ring.style.opacity = "0";
        ring.style.transform = "translate(" + state.x + "px," + state.y + "px) scale(1.4)";
      });
    },
    center(selector, text) {
      const nodes = [...document.querySelectorAll(selector)];
      const el = text ? nodes.find((n) => (n.innerText || "").trim().startsWith(text)) : nodes[0];
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    },
    fire(selector, text, kind) {
      const nodes = [...document.querySelectorAll(selector)];
      const el = text ? nodes.find((n) => (n.innerText || "").trim().startsWith(text)) : nodes[0];
      if (!el) return "missing:" + (text || selector);
      const r = el.getBoundingClientRect();
      const x = r.left + r.width / 2;
      const y = r.top + r.height / 2;
      const base = { bubbles: true, cancelable: true, clientX: x, clientY: y, view: window,
                     pointerId: 1, isPrimary: true, pointerType: "mouse" };
      if (kind === "context") {
        el.dispatchEvent(new MouseEvent("contextmenu", { ...base, button: 2 }));
        return "ok";
      }
      el.dispatchEvent(new PointerEvent("pointerover", base));
      el.dispatchEvent(new MouseEvent("mouseover", base));
      el.dispatchEvent(new MouseEvent("mousemove", base));
      if (kind === "hover") return "ok";
      el.dispatchEvent(new PointerEvent("pointerdown", { ...base, button: 0 }));
      el.dispatchEvent(new MouseEvent("mousedown", { ...base, button: 0 }));
      el.dispatchEvent(new PointerEvent("pointerup", { ...base, button: 0 }));
      el.dispatchEvent(new MouseEvent("mouseup", { ...base, button: 0 }));
      el.dispatchEvent(new MouseEvent("click", { ...base, button: 0 }));
      return "ok";
    },
    // Dispatched on the focused element: the palette listens on its own input,
    // and a document-level event never reaches it.
    key(key, opts) {
      const init = { key, code: opts?.code ?? "Key" + key.toUpperCase(), bubbles: true,
                     cancelable: true, ...opts };
      const target = document.activeElement ?? document;
      target.dispatchEvent(new KeyboardEvent("keydown", init));
      target.dispatchEvent(new KeyboardEvent("keyup", init));
      return "ok";
    },
    type(selector, text) {
      const el = document.querySelector(selector);
      if (!el) return "missing";
      el.focus();
      el.value = text;
      el.dispatchEvent(new Event("input", { bubbles: true }));
      return "ok";
    },
  };
  return "ready";
})()
`;

// --------------------------------------------------------------------- shots

const SHOTS = {
  // The grid, with the pointer drifting in. Sets the scene.
  overview: async (a) => {
    await a.cursorAt(".card", "main", 0);
    await a.cursorShow();
    await a.wait(700);
    await a.hover(".card", "bro's account", 700);
    await a.wait(900);
  },

  // Two clicks: the first arms the on-card confirmation, the second switches.
  switch: async (a) => {
    await a.cursorAt(".card", "main", 0);
    await a.cursorShow();
    await a.wait(400);
    await a.click(".card", "bro's account", 650);
    await a.wait(950);
    await a.click(".card", "bro's account", 0);
    await a.wait(2600);
  },

  // Ctrl+K, type, switch from the palette without touching the grid. "bro"
  // rather than a smurf so the card behind the palette visibly flips to active.
  palette: async (a) => {
    await a.cursorShow(false);
    await a.wait(500);
    await a.key("k", { ctrlKey: true });
    await a.wait(900);
    await a.typeSlowly("input[placeholder*='command']", "smurf", 190);
    await a.wait(1400);
    await a.eraseSlowly("input[placeholder*='command']", "smurf", 110);
    await a.typeSlowly("input[placeholder*='command']", "bro", 190);
    await a.wait(1200);
    await a.key("Enter", { code: "Enter" });
    await a.wait(2600);
  },

  // Right-click an account, walk into the appearance submenu, recolor the card.
  color: async (a) => {
    await a.cursorAt(".card", "main", 0);
    await a.cursorShow();
    await a.wait(500);
    await a.rightClick(".card", "main", 600);
    await a.wait(900);
    await a.click(".menu-item", "Edit card", 700);
    await a.wait(900);
    await a.clickByTitle("button.swatch", "Violet", 700);
    await a.wait(1400);
    await a.key("Escape", { code: "Escape" });
    await a.wait(900);
  },

  // Folder in, folder out.
  folder: async (a) => {
    await a.cursorAt(".card", "Smurfs", 0);
    await a.cursorShow();
    await a.wait(400);
    await a.click(".card", "Smurfs", 600);
    await a.wait(1800);
    await a.click(".card", "Back", 700);
    await a.wait(1500);
  },

  // Settings, theme swatches, back out.
  theme: async (a) => {
    await a.cursorShow();
    await a.click('button[title="Settings"]', null, 700);
    await a.wait(1600);
    await a.clickByTitle(".theme-swatch", "Light", 700);
    await a.wait(1500);
    await a.clickByTitle(".theme-swatch", "Midnight", 600);
    await a.wait(1400);
    await a.clickByTitle(".theme-swatch", "Dark", 600);
    await a.wait(1200);
    await a.key("Escape", { code: "Escape" });
    await a.wait(1000);
  },

  // Same app, another platform.
  riot: async (a) => {
    await a.cursorShow();
    await a.wait(400);
    await a.click('button.tab[aria-label*="Riot"]', null, 700);
    await a.wait(2200);
    await a.hover(".card", "smurf", 600);
    await a.wait(1000);
  },
};

// ------------------------------------------------------------------ recording

async function record(shotName) {
  const outDir = join(ROOT, shotName);
  rmSync(outDir, { recursive: true, force: true });
  mkdirSync(outDir, { recursive: true });

  const shots = await connect();
  const driver = await connect();

  const js = async (script) => {
    const res = await driver.send("execute_js", { script });
    if (!res.success) throw new Error(`execute_js failed: ${JSON.stringify(res).slice(0, 200)}`);
    return res.data;
  };

  // Every shot starts from a clean app: the demo module keeps the current
  // account and card colors in module scope, so a reload resets them.
  await js("location.reload(); 'ok'");
  await sleep(2600);
  await js(HELPERS);

  let frame = 0;
  const timeline = [];
  let recording = true;

  const loop = (async () => {
    while (recording) {
      const started = Date.now();
      try {
        const res = await shots.send("capture_native_screenshot", { format: "png" });
        if (res.success && typeof res.data === "string") {
          const base64 = res.data.slice(res.data.indexOf(",") + 1);
          const name = `f${String(frame++).padStart(4, "0")}.png`;
          writeFileSync(join(outDir, name), Buffer.from(base64, "base64"));
          timeline.push({ name, at: started });
        }
      } catch (err) {
        console.error("frame failed:", err.message);
      }
      const spent = Date.now() - started;
      if (spent < FRAME_INTERVAL_MS) await sleep(FRAME_INTERVAL_MS - spent);
    }
  })();

  const q = (v) => JSON.stringify(v ?? null);
  const api = {
    wait: sleep,
    cursorShow: async (on = true) => {
      await js(`window.__demo.cursorShow(${on}); "ok"`);
      await sleep(260);
    },
    cursorAt: async (selector, text, travelMs = 550) => {
      await js(
        `(async () => { const p = window.__demo.center(${q(selector)}, ${q(text)});` +
          ` if (!p) return "missing"; await window.__demo.moveTo(p.x, p.y, ${travelMs}); return "ok"; })()`,
      );
      await sleep(travelMs + 60);
    },
    hover: async (selector, text, travelMs = 550) => {
      await api.cursorAt(selector, text, travelMs);
      await js(`window.__demo.fire(${q(selector)}, ${q(text)}, "hover")`);
    },
    click: async (selector, text, travelMs = 550) => {
      await api.cursorAt(selector, text, travelMs);
      await js(`window.__demo.clickFx(); window.__demo.fire(${q(selector)}, ${q(text)}, "click")`);
    },
    rightClick: async (selector, text, travelMs = 550) => {
      await api.cursorAt(selector, text, travelMs);
      await js(
        `window.__demo.clickFx(); window.__demo.fire(${q(selector)}, ${q(text)}, "context")`,
      );
    },
    // Colour swatches and theme tiles carry a title but no text of their own.
    clickByTitle: async (selector, title, travelMs = 550) => {
      const res = await js(
        `(async () => { const el = document.querySelector(${q(selector)} + '[title=' + JSON.stringify(${q(title)}) + ']');` +
          ` if (!el) return "missing"; const r = el.getBoundingClientRect();` +
          ` await window.__demo.moveTo(r.left + r.width / 2, r.top + r.height / 2, ${travelMs});` +
          ` window.__demo.clickFx(); el.click(); return "ok"; })()`,
      );
      if (res === "missing") console.error(`missing ${selector}[title=${title}]`);
      await sleep(travelMs + 120);
    },
    key: async (key, opts) => {
      await js(`window.__demo.key(${q(key)}, ${JSON.stringify(opts ?? {})})`);
      await sleep(120);
    },
    // Character by character, so the palette filtering is readable.
    typeSlowly: async (selector, text, perChar = 170) => {
      for (let i = 1; i <= text.length; i++) {
        await js(`window.__demo.type(${q(selector)}, ${q(text.slice(0, i))})`);
        await sleep(perChar);
      }
    },
    eraseSlowly: async (selector, text, perChar = 110) => {
      for (let i = text.length - 1; i >= 0; i--) {
        await js(`window.__demo.type(${q(selector)}, ${q(text.slice(0, i))})`);
        await sleep(perChar);
      }
    },
  };

  try {
    await SHOTS[shotName](api);
  } finally {
    recording = false;
    await loop;
  }

  writeFileSync(join(outDir, "timeline.json"), JSON.stringify(timeline, null, 1));
  const lines = [];
  for (let i = 0; i < timeline.length; i++) {
    const next = timeline[i + 1];
    const duration = next ? (next.at - timeline[i].at) / 1000 : FRAME_INTERVAL_MS / 1000;
    lines.push(`file '${timeline[i].name}'`, `duration ${duration.toFixed(3)}`);
  }
  if (timeline.length) lines.push(`file '${timeline[timeline.length - 1].name}'`);
  writeFileSync(join(outDir, "frames.txt"), lines.join("\n") + "\n");

  const span = timeline.length ? (timeline.at(-1).at - timeline[0].at) / 1000 : 0;
  console.log(
    `${shotName}: ${timeline.length} frames, ${span.toFixed(1)}s (${(timeline.length / (span || 1)).toFixed(1)} fps)`,
  );

  shots.close();
  driver.close();
}

// -------------------------------------------------------------------- render

// The cut. Three short clips rather than one long reel: a README reads better
// with a few focused loops, and each one keeps its own 256-colour palette
// instead of sharing a stretched one across every scene.
// `speed` below 1 plays faster (0.8 = 1.25x), which trims the dead air a
// scripted take always carries.
const CLIPS = [
  {
    name: "demo-switch",
    cuts: [
      { shot: "switch", start: 0.4, duration: 3.6 },
      { shot: "palette", start: 0.6, duration: 5.6 },
    ],
  },
  {
    name: "demo-organize",
    cuts: [
      { shot: "color", start: 0.4, duration: 7.0 },
      { shot: "folder", start: 0.3, duration: 3.4 },
    ],
  },
  {
    name: "demo-themes",
    cuts: [
      { shot: "riot", start: 0.5, duration: 3.0 },
      { shot: "theme", start: 0.6, duration: 5.0 },
    ],
  },
];
const SPEED = 0.8;
// Native window width, 15 fps and a full palette. Each clip is short enough
// that the extra weight stays reasonable.
const GIF_FPS = 15;
const GIF_WIDTH = 940;
const GIF_COLORS = 256;

function render() {
  for (const clip of CLIPS) {
    const parts = [];
    for (const cut of clip.cuts) {
      const dir = join(ROOT, cut.shot);
      if (!existsSync(join(dir, "frames.txt"))) {
        console.error(`skipping ${cut.shot}: not recorded`);
        continue;
      }
      execFileSync(
        FFMPEG,
        [
          "-hide_banner",
          "-loglevel",
          "error",
          "-f",
          "concat",
          "-safe",
          "0",
          "-i",
          "frames.txt",
          "-ss",
          String(cut.start),
          "-t",
          String(cut.duration),
          "-vf",
          `setpts=${SPEED}*PTS,fps=25,scale=trunc(iw/2)*2:trunc(ih/2)*2`,
          "-c:v",
          "libx264",
          "-crf",
          "18",
          "-preset",
          "medium",
          "-pix_fmt",
          "yuv420p",
          "-an",
          "-y",
          join("..", `${cut.shot}.mp4`),
        ],
        { cwd: dir, stdio: "inherit" },
      );
      parts.push(`${cut.shot}.mp4`);
    }
    if (!parts.length) {
      console.error(`skipping clip ${clip.name}: nothing recorded`);
      continue;
    }

    const listName = `${clip.name}.txt`;
    writeFileSync(join(ROOT, listName), parts.map((f) => `file '${f}'`).join("\n") + "\n");

    const mp4 = resolve(`docs/${clip.name}.mp4`);
    execFileSync(
      FFMPEG,
      [
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        listName,
        "-vf",
        "fade=t=in:st=0:d=0.3",
        "-c:v",
        "libx264",
        "-crf",
        "18",
        "-preset",
        "slow",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
        "-an",
        "-y",
        mp4,
      ],
      { cwd: ROOT, stdio: "inherit" },
    );

    // Per-clip palette, no dither: the UI is flat fills, and 256 colours cover
    // the avatar gradients without the noise dithering would add.
    execFileSync(
      FFMPEG,
      [
        "-hide_banner",
        "-loglevel",
        "error",
        "-i",
        mp4,
        "-vf",
        `fps=${GIF_FPS},scale=${GIF_WIDTH}:-2:flags=lanczos,split[a][b];[a]palettegen=max_colors=${GIF_COLORS}:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle`,
        "-loop",
        "0",
        "-y",
        resolve(`docs/${clip.name}.gif`),
      ],
      { cwd: ROOT, stdio: "inherit" },
    );

    console.log(`wrote docs/${clip.name}.gif`);
  }
}

// ---------------------------------------------------------------------- main

const arg = process.argv[2];
if (!arg) {
  console.error(
    `usage: node scripts/record-demo.mjs <${Object.keys(SHOTS).join("|")}|all|--render>`,
  );
  process.exit(1);
}

if (arg === "--render") {
  render();
} else if (arg === "all") {
  for (const name of Object.keys(SHOTS)) {
    await record(name);
  }
} else if (SHOTS[arg]) {
  await record(arg);
} else {
  console.error(`unknown shot: ${arg}`);
  process.exit(1);
}
