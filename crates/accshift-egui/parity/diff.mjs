// Pixel diff of the parity bench: reference frames (ref.mjs) against egui
// frames. Node and ffmpeg only (ffmpeg on PATH, or FFMPEG=<path>).
//
//   node diff.mjs <ref.png> <other.png|pam> --out <dir>
//   node diff.mjs --batch <refDir> <otherDir> --out <dir>
//
// Both images are decoded to raw RGBA by ffmpeg (PNG or PAM in), then each is
// composited over black so a transparent pixel compares as what an opaque
// window would show. Per shot:
//   mae      mean absolute error over R, G and B, in 0-255 levels
//   psnr     10 log10(255^2 / MSE) over R, G and B, in dB (99 when identical)
//   over8    percent of pixels whose largest channel delta exceeds 8
//   over24   same, above 24
//   worst    the 16x16 tile (aligned on a 16 px grid, in image pixels) with the
//            highest mean error: x, y and its mae
// and two PNGs: <shot>-heat.png (largest channel delta per pixel, x4, on a
// black to red to yellow to white ramp) and <shot>-side.png (reference on
// top, other below). When the sizes differ, the overlapping top-left area is
// compared and the shot is flagged.
//
// Batch mode pairs shots through <refDir>/manifest.json (written by ref.mjs):
// <otherDir>/<scene>/<shot>.png|pam first, then <otherDir>/<alias>.png|pam for
// shots whose scene step names an alias (the spike's `--capture` PAM names).
// Without a manifest, every <refDir>/<scene>/<shot>.png is tried. Writes
// <out>/report.json and <out>/report.md, one row per shot, and the images
// under <out>/<scene>/.

import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, extname, join, relative, resolve } from "node:path";

const FFMPEG = process.env.FFMPEG ?? "ffmpeg";

function headerSize(file) {
  const buf = readFileSync(file);
  if (buf.subarray(1, 4).toString("latin1") === "PNG") {
    return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) };
  }
  if (buf.subarray(0, 2).toString("latin1") === "P7") {
    const head = buf.subarray(0, 256).toString("latin1");
    const width = Number(head.match(/WIDTH (\d+)/)?.[1]);
    const height = Number(head.match(/HEIGHT (\d+)/)?.[1]);
    if (width && height) return { width, height };
  }
  throw new Error(`${file}: not a PNG or PAM`);
}

// ffmpeg takes about two seconds to start on Windows, so each shot costs two
// runs: one decodes both inputs, one encodes both outputs. Raw frames go
// through a temp folder.
function withTemp(job) {
  const dir = mkdtempSync(join(tmpdir(), "parity-diff-"));
  try {
    return job(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** RGBA pixels of PNG or PAM files, through one ffmpeg run. */
export function decode(...files) {
  return withTemp((dir) => {
    const args = ["-v", "error", "-y"];
    for (const f of files) args.push("-i", f);
    files.forEach((_, i) =>
      args.push("-map", `${i}:v`, "-f", "rawvideo", "-pix_fmt", "rgba", join(dir, `${i}.rgba`)),
    );
    execFileSync(FFMPEG, args, { windowsHide: true, stdio: ["ignore", "ignore", "pipe"] });
    return files.map((file, i) => {
      const { width, height } = headerSize(file);
      const data = readFileSync(join(dir, `${i}.rgba`));
      if (data.length !== width * height * 4) {
        throw new Error(`${file}: ffmpeg gave ${data.length} bytes for ${width}x${height}`);
      }
      return { width, height, data };
    });
  });
}

/** Write RGBA buffers as PNGs, through one ffmpeg run. */
function encodePngs(images) {
  withTemp((dir) => {
    const args = ["-v", "error", "-y"];
    images.forEach(({ file, width, height, rgba }, i) => {
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(join(dir, `${i}.rgba`), rgba);
      args.push(
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
        `${width}x${height}`,
        "-i",
        join(dir, `${i}.rgba`),
      );
    });
    images.forEach(({ file }, i) => args.push("-map", `${i}:v`, "-frames:v", "1", file));
    execFileSync(FFMPEG, args, { windowsHide: true, stdio: ["ignore", "ignore", "pipe"] });
  });
}

/** RGB of every pixel, composited over black, 3 bytes a pixel. */
function overBlack(img) {
  const n = img.width * img.height;
  const rgb = new Uint8Array(n * 3);
  for (let i = 0; i < n; i++) {
    const alpha = img.data[i * 4 + 3];
    for (let c = 0; c < 3; c++) rgb[i * 3 + c] = Math.round((img.data[i * 4 + c] * alpha) / 255);
  }
  return rgb;
}

function ramp(v) {
  // 0 black, 1/3 red, 2/3 yellow, 1 white.
  const t = Math.min(1, v / 255);
  const r = Math.min(1, t * 3);
  const g = Math.min(1, Math.max(0, t * 3 - 1));
  const b = Math.min(1, Math.max(0, t * 3 - 2));
  return [r * 255, g * 255, b * 255];
}

export function compare(refFile, otherFile, heatFile, sideFile) {
  const [a, b] = decode(refFile, otherFile);
  a.rgb = overBlack(a);
  b.rgb = overBlack(b);
  const width = Math.min(a.width, b.width);
  const height = Math.min(a.height, b.height);
  const sizeMatch = a.width === b.width && a.height === b.height;
  const heat = Buffer.alloc(width * height * 4);
  const tilesX = Math.ceil(width / 16);
  const tilesY = Math.ceil(height / 16);
  const tileSum = new Float64Array(tilesX * tilesY);
  const tileCount = new Uint32Array(tilesX * tilesY);
  let absSum = 0;
  let sqSum = 0;
  let over8 = 0;
  let over24 = 0;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const ia = y * a.width + x;
      const ib = y * b.width + x;
      let max = 0;
      let abs = 0;
      for (let c = 0; c < 3; c++) {
        const d = Math.abs(a.rgb[ia * 3 + c] - b.rgb[ib * 3 + c]);
        abs += d;
        sqSum += d * d;
        if (d > max) max = d;
      }
      absSum += abs;
      if (max > 8) over8++;
      if (max > 24) over24++;
      const t = Math.floor(y / 16) * tilesX + Math.floor(x / 16);
      tileSum[t] += abs / 3;
      tileCount[t]++;
      const [r, g, bl] = ramp(max * 4);
      const o = (y * width + x) * 4;
      heat[o] = r;
      heat[o + 1] = g;
      heat[o + 2] = bl;
      heat[o + 3] = 255;
    }
  }
  const n = width * height;
  const mse = sqSum / (n * 3);
  let worst = 0;
  for (let t = 1; t < tileSum.length; t++) {
    if (tileSum[t] / tileCount[t] > tileSum[worst] / tileCount[worst]) worst = t;
  }

  // Reference on top, other below, both over black, on the wider canvas.
  const sw = Math.max(a.width, b.width);
  const side = Buffer.alloc(sw * (a.height + b.height) * 4);
  const blit = (img, top) => {
    for (let y = 0; y < img.height; y++) {
      for (let x = 0; x < img.width; x++) {
        const i = y * img.width + x;
        const o = ((top + y) * sw + x) * 4;
        side[o] = img.rgb[i * 3];
        side[o + 1] = img.rgb[i * 3 + 1];
        side[o + 2] = img.rgb[i * 3 + 2];
        side[o + 3] = 255;
      }
    }
  };
  blit(a, 0);
  blit(b, a.height);
  encodePngs([
    { file: heatFile, width, height, rgba: heat },
    { file: sideFile, width: sw, height: a.height + b.height, rgba: side },
  ]);

  const round = (v, d = 2) => Math.round(v * 10 ** d) / 10 ** d;
  return {
    width,
    height,
    sizeMatch,
    refSize: [a.width, a.height],
    otherSize: [b.width, b.height],
    mae: round(absSum / (n * 3), 3),
    psnr: mse === 0 ? 99 : round(10 * Math.log10((255 * 255) / mse)),
    over8: round((over8 / n) * 100),
    over24: round((over24 / n) * 100),
    worst: {
      x: (worst % tilesX) * 16,
      y: Math.floor(worst / tilesX) * 16,
      mae: round(tileSum[worst] / tileCount[worst]),
    },
  };
}

function findOther(otherDir, stems) {
  for (const stem of stems) {
    for (const ext of [".png", ".pam"]) {
      const p = join(otherDir, stem + ext);
      if (existsSync(p)) return p;
    }
  }
  return null;
}

function refShots(refDir) {
  const manifest = join(refDir, "manifest.json");
  if (existsSync(manifest)) return JSON.parse(readFileSync(manifest, "utf8"));
  const shots = [];
  for (const scene of readdirSync(refDir)) {
    const dir = join(refDir, scene);
    if (!statSync(dir).isDirectory() || scene.startsWith(".")) continue;
    for (const f of readdirSync(dir)) {
      if (extname(f) === ".png")
        shots.push({ scene, shot: basename(f, ".png"), file: `${scene}/${f}` });
    }
  }
  return shots;
}

function batch(refDir, otherDir, out) {
  const rows = [];
  for (const s of refShots(refDir)) {
    const other = findOther(otherDir, [`${s.scene}/${s.shot}`, ...(s.alias ? [s.alias] : [])]);
    const row = { scene: s.scene, shot: s.shot, ref: join(refDir, s.file), other };
    if (!other) {
      row.missing = true;
      console.log(`${s.scene}/${s.shot}: no counterpart in ${otherDir}`);
    } else {
      Object.assign(
        row,
        compare(
          row.ref,
          other,
          join(out, s.scene, `${s.shot}-heat.png`),
          join(out, s.scene, `${s.shot}-side.png`),
        ),
      );
      console.log(
        `${s.scene}/${s.shot}: psnr ${row.psnr} dB, mae ${row.mae}, >8 ${row.over8}%, >24 ${row.over24}%`,
      );
    }
    rows.push(row);
  }
  const done = rows.filter((r) => !r.missing);
  const mean = (k) =>
    done.length ? Math.round((done.reduce((s, r) => s + r[k], 0) / done.length) * 100) / 100 : null;
  const summary = {
    shots: rows.length,
    compared: done.length,
    meanPsnr: mean("psnr"),
    meanMae: mean("mae"),
  };
  writeFileSync(
    join(out, "report.json"),
    JSON.stringify({ refDir, otherDir, summary, rows }, null, 2) + "\n",
  );

  const rel = (p) => relative(out, p).split("\\").join("/");
  const md = [
    `# Parity diff`,
    "",
    `Reference \`${refDir}\` against \`${otherDir}\`: ${done.length} of ${rows.length} shots compared, mean PSNR ${summary.meanPsnr} dB, mean MAE ${summary.meanMae}.`,
    "",
    "| Shot | Other | PSNR (dB) | MAE | > 8 (%) | > 24 (%) | Worst 16x16 tile | Images |",
    "| --- | --- | ---: | ---: | ---: | ---: | --- | --- |",
    ...rows.map((r) =>
      r.missing
        ? `| ${r.scene}/${r.shot} | missing | | | | | | |`
        : `| ${r.scene}/${r.shot}${r.sizeMatch ? "" : ` (size ${r.refSize.join("x")} vs ${r.otherSize.join("x")})`} | ${basename(r.other)} | ${r.psnr} | ${r.mae} | ${r.over8} | ${r.over24} | ${r.worst.x},${r.worst.y} (mae ${r.worst.mae}) | [heat](${rel(join(out, r.scene, `${r.shot}-heat.png`))}), [side](${rel(join(out, r.scene, `${r.shot}-side.png`))}) |`,
    ),
    "",
  ].join("\n");
  writeFileSync(join(out, "report.md"), md);
  console.log(
    `${done.length}/${rows.length} shots, mean PSNR ${summary.meanPsnr} dB -> ${join(out, "report.md")}`,
  );
  return summary;
}

function main() {
  const argv = process.argv.slice(2);
  let out = null;
  let isBatch = false;
  const pos = [];
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--out") out = argv[++i];
    else if (argv[i] === "--batch") isBatch = true;
    else pos.push(argv[i]);
  }
  if (!out || pos.length !== 2) {
    console.error(
      "usage: diff.mjs <ref> <other> --out <dir> | diff.mjs --batch <refDir> <otherDir> --out <dir>",
    );
    process.exit(2);
  }
  out = resolve(out);
  mkdirSync(out, { recursive: true });
  const [ref, other] = pos.map((p) => resolve(p));
  if (isBatch) {
    const summary = batch(ref, other, out);
    if (summary.compared === 0) process.exitCode = 1;
    return;
  }
  const stem = basename(ref, extname(ref));
  const result = compare(ref, other, join(out, `${stem}-heat.png`), join(out, `${stem}-side.png`));
  console.log(JSON.stringify(result));
}

main();
