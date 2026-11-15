// The dependency lists of the deb and the rpm, set back to tauri.conf.json.
//
// On Linux the Tauri CLI appends libwebkit2gtk-4.1 and libgtk-3 to both
// lists, unconditionally (crates/tauri-cli/src/interface/rust.rs): the
// webview a Tauri app runs in. The egui app loads neither, and a fresh
// install would pull about 100 MB for nothing. The packages are rewritten
// after the bundler with the lists of the config alone; only the control
// file of the deb and the require tags of the rpm change, the payloads are
// the same bytes.
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync, gzipSync } from "node:zlib";

// --- deb: an ar archive holding debian-binary, control.tar.gz, data.tar.*

const AR_MAGIC = "!<arch>\n";

function readAr(buffer) {
  if (buffer.toString("latin1", 0, 8) !== AR_MAGIC) throw new Error("not an ar archive");
  const members = [];
  let at = 8;
  while (at < buffer.length) {
    const header = buffer.subarray(at, at + 60);
    const size = Number.parseInt(header.toString("latin1", 48, 58).trim(), 10);
    members.push({ header: Buffer.from(header), data: buffer.subarray(at + 60, at + 60 + size) });
    at += 60 + size + (size % 2);
  }
  return members;
}

function writeAr(members) {
  const parts = [Buffer.from(AR_MAGIC, "latin1")];
  for (const { header, data } of members) {
    header.write(String(data.length).padEnd(10, " "), 48, 10, "latin1");
    parts.push(header, data);
    if (data.length % 2) parts.push(Buffer.from("\n"));
  }
  return Buffer.concat(parts);
}

function octal(header, offset, length, value) {
  header.write(`${value.toString(8).padStart(length - 1, "0")}\0`, offset, length, "latin1");
}

// Replaces one regular file of an uncompressed tar, other entries untouched.
function replaceInTar(tar, name, content) {
  const blocks = [];
  let at = 0;
  let found = false;
  while (at + 512 <= tar.length) {
    const header = Buffer.from(tar.subarray(at, at + 512));
    if (header.every((byte) => byte === 0)) {
      blocks.push(tar.subarray(at));
      break;
    }
    const size = Number.parseInt(header.toString("latin1", 124, 136).replace(/\0.*$/s, "").trim() || "0", 8);
    const padded = Math.ceil(size / 512) * 512;
    let data = tar.subarray(at + 512, at + 512 + padded);
    const entry = header.toString("latin1", 0, 100).replace(/\0.*$/s, "");
    if (entry.replace(/^\.\//, "") === name) {
      found = true;
      octal(header, 124, 12, content.length);
      header.fill(0x20, 148, 156);
      octal(header, 148, 7, header.reduce((sum, byte) => sum + byte, 0));
      header[155] = 0x20;
      data = Buffer.alloc(Math.ceil(content.length / 512) * 512);
      content.copy(data);
    }
    blocks.push(header, data);
    at += 512 + padded;
  }
  if (!found) throw new Error(`no ${name} in the control archive`);
  return Buffer.concat(blocks);
}

// The field keeps its place; a control file without one gets it before the
// description.
function controlWithDepends(control, depends) {
  const lines = control.split("\n");
  let at = lines.findIndex((line) => line.startsWith("Depends:"));
  if (at >= 0) lines.splice(at, 1);
  else at = Math.max(0, lines.findIndex((line) => line.startsWith("Description:")));
  if (depends.length) lines.splice(at, 0, `Depends: ${depends.join(", ")}`);
  return lines.join("\n");
}

function debControl(members) {
  const member = members.find((m) => m.header.toString("latin1", 0, 16).trim().replace(/\/$/, "") === "control.tar.gz");
  if (!member) throw new Error("no control.tar.gz in the deb");
  return member;
}

function readTarFile(tar, name) {
  let at = 0;
  while (at + 512 <= tar.length) {
    const entry = tar.toString("latin1", at, at + 100).replace(/\0.*$/s, "");
    if (!entry) break;
    const size = Number.parseInt(tar.toString("latin1", at + 124, at + 136).replace(/\0.*$/s, "").trim() || "0", 8);
    if (entry.replace(/^\.\//, "") === name) return tar.subarray(at + 512, at + 512 + size).toString("utf8");
    at += 512 + Math.ceil(size / 512) * 512;
  }
  throw new Error(`no ${name} in the control archive`);
}

/** The `Depends` field of a deb, `[]` when it has none. */
export function debDepends(file) {
  const control = readTarFile(gunzipSync(debControl(readAr(readFileSync(file))).data), "control");
  const line = control.split("\n").find((l) => l.startsWith("Depends:"));
  return line ? line.slice("Depends:".length).split(",").map((d) => d.trim()) : [];
}

/** Sets the `Depends` field of a deb to `depends`. */
export function setDebDepends(file, depends) {
  const members = readAr(readFileSync(file));
  const member = debControl(members);
  const tar = gunzipSync(member.data);
  const control = controlWithDepends(readTarFile(tar, "control"), depends);
  member.data = gzipSync(replaceInTar(tar, "control", Buffer.from(control, "utf8")), { level: 9 });
  writeFileSync(file, writeAr(members));
  const now = debDepends(file);
  if (now.join(", ") !== depends.join(", ")) throw new Error(`${file}: Depends is ${now.join(", ")}`);
}

// --- rpm: lead (96 bytes), signature header (padded to 8), header, payload

const HEADER_MAGIC = Buffer.from([0x8e, 0xad, 0xe8, 0x01, 0, 0, 0, 0]);
const STRING = 6;
const BIN = 7;
const STRING_ARRAY = 8;
const I18NSTRING = 9;
const SIZES = { 0: 0, 1: 1, 2: 1, 3: 2, 4: 4, 5: 8 };

const REQUIRE_FLAGS = 1048;
const REQUIRE_NAME = 1049;
const REQUIRE_VERSION = 1050;

// The signature tags that cover the header: digests and sizes are computed
// again; a cryptographic signature cannot be, so the rewrite refuses it.
const SIG = { size: 1000, longSize: 270, md5: 1004, sha1: 269, sha256: 273, sha3: 279 };
const SIG_KEPT = new Set([62, 999, 1007, 1008, 271]);

function readHeader(buffer, at) {
  if (!buffer.subarray(at, at + 8).equals(HEADER_MAGIC)) throw new Error(`no rpm header at ${at}`);
  const count = buffer.readUInt32BE(at + 8);
  const storeSize = buffer.readUInt32BE(at + 12);
  const store = buffer.subarray(at + 16 + count * 16, at + 16 + count * 16 + storeSize);
  const entries = [];
  for (let i = 0; i < count; i += 1) {
    const e = at + 16 + i * 16;
    entries.push({
      tag: buffer.readUInt32BE(e),
      type: buffer.readUInt32BE(e + 4),
      offset: buffer.readInt32BE(e + 8),
      count: buffer.readUInt32BE(e + 12),
    });
  }
  return { entries, store, end: at + 16 + count * 16 + storeSize };
}

function strings(store, offset, count) {
  const out = [];
  let at = offset;
  for (let i = 0; i < count; i += 1) {
    const end = store.indexOf(0, at);
    out.push(store.toString("utf8", at, end));
    at = end + 1;
  }
  return { values: out, length: at - offset };
}

function entryData(store, entry) {
  if (entry.type === STRING) return store.subarray(entry.offset, store.indexOf(0, entry.offset) + 1);
  if (entry.type === STRING_ARRAY || entry.type === I18NSTRING) {
    return store.subarray(entry.offset, entry.offset + strings(store, entry.offset, entry.count).length);
  }
  if (entry.type === BIN) return store.subarray(entry.offset, entry.offset + entry.count);
  return store.subarray(entry.offset, entry.offset + SIZES[entry.type] * entry.count);
}

function writeHeader(entries) {
  // Data in the order of the original offsets, each type at its alignment.
  const order = [...entries].sort((a, b) => a.offset - b.offset);
  const parts = [];
  let size = 0;
  for (const entry of order) {
    const align = { 3: 2, 4: 4, 5: 8 }[entry.type] ?? 1;
    const pad = (align - (size % align)) % align;
    if (pad) parts.push(Buffer.alloc(pad));
    size += pad;
    entry.newOffset = size;
    parts.push(entry.data);
    size += entry.data.length;
  }
  const index = Buffer.alloc(16 + entries.length * 16);
  HEADER_MAGIC.copy(index);
  index.writeUInt32BE(entries.length, 8);
  index.writeUInt32BE(size, 12);
  entries.forEach((entry, i) => {
    index.writeUInt32BE(entry.tag, 16 + i * 16);
    index.writeUInt32BE(entry.type, 20 + i * 16);
    index.writeInt32BE(entry.newOffset, 24 + i * 16);
    index.writeUInt32BE(entry.count, 28 + i * 16);
  });
  return Buffer.concat([index, ...parts]);
}

function rpmParts(buffer) {
  const sig = readHeader(buffer, 96);
  const headerAt = sig.end + ((8 - (sig.end % 8)) % 8);
  const header = readHeader(buffer, headerAt);
  return { sig, sigAt: 96, headerAt, header, payload: buffer.subarray(header.end) };
}

/** The `Requires` names of an rpm, in order. */
export function rpmRequires(file) {
  const { header } = rpmParts(readFileSync(file));
  const name = header.entries.find((e) => e.tag === REQUIRE_NAME);
  return name ? strings(header.store, name.offset, name.count).values : [];
}

/**
 * Removes the `Requires` entries for which `drop(name)` is true, then
 * recomputes the digests and the size the signature header keeps of the
 * header. The signature header keeps its size: only values change in place.
 */
export function dropRpmRequires(file, drop) {
  const buffer = Buffer.from(readFileSync(file));
  const { sig, sigAt, headerAt, header, payload } = rpmParts(buffer);
  for (const entry of header.entries) entry.data = entryData(header.store, entry);

  const names = header.entries.find((e) => e.tag === REQUIRE_NAME);
  if (!names) return;
  const keep = strings(header.store, names.offset, names.count).values.map((name) => !drop(name));
  if (keep.every(Boolean)) return;
  for (const tag of [REQUIRE_NAME, REQUIRE_FLAGS, REQUIRE_VERSION]) {
    const entry = header.entries.find((e) => e.tag === tag);
    if (!entry) continue;
    if (entry.count !== keep.length) throw new Error(`${file}: tag ${tag} has ${entry.count} values, not ${keep.length}`);
    if (entry.type === STRING_ARRAY) {
      const values = strings(header.store, entry.offset, entry.count).values.filter((_, i) => keep[i]);
      entry.data = Buffer.concat(values.map((v) => Buffer.from(`${v}\0`, "utf8")));
    } else if (entry.type === 4) {
      const values = [];
      for (let i = 0; i < entry.count; i += 1) if (keep[i]) values.push(entry.data.readUInt32BE(i * 4));
      entry.data = Buffer.alloc(values.length * 4);
      values.forEach((v, i) => entry.data.writeUInt32BE(v, i * 4));
    } else {
      throw new Error(`${file}: unexpected type ${entry.type} for tag ${tag}`);
    }
    entry.count = keep.filter(Boolean).length;
  }
  const newHeader = writeHeader(header.entries);

  // The signature header, patched in place.
  const sigStore = buffer.subarray(sigAt + 16 + sig.entries.length * 16, sig.end);
  const hex = (algorithm, ...parts) => {
    const hash = createHash(algorithm);
    for (const part of parts) hash.update(part);
    return hash.digest("hex");
  };
  for (const entry of sig.entries) {
    const put = (value) => {
      const old = entryData(sigStore, entry);
      if (old.length !== value.length) throw new Error(`${file}: signature tag ${entry.tag} changes size`);
      value.copy(sigStore, entry.offset);
    };
    if (entry.tag === SIG.size) {
      sigStore.writeUInt32BE(newHeader.length + payload.length, entry.offset);
    } else if (entry.tag === SIG.longSize) {
      sigStore.writeBigUInt64BE(BigInt(newHeader.length + payload.length), entry.offset);
    } else if (entry.tag === SIG.md5) {
      put(createHash("md5").update(newHeader).update(payload).digest());
    } else if (entry.tag === SIG.sha1) {
      put(Buffer.from(`${hex("sha1", newHeader)}\0`, "latin1"));
    } else if (entry.tag === SIG.sha256) {
      put(Buffer.from(`${hex("sha256", newHeader)}\0`, "latin1"));
    } else if (entry.tag === SIG.sha3) {
      put(Buffer.from(`${hex("sha3-256", newHeader)}\0`, "latin1"));
    } else if (!SIG_KEPT.has(entry.tag)) {
      throw new Error(`${file}: signature tag ${entry.tag} covers the header and cannot be computed again`);
    }
  }
  writeFileSync(file, Buffer.concat([buffer.subarray(0, headerAt), newHeader, payload]));
}

// --- what the rewrite touches and how it signs

/**
 * The packages of `version` in `dir` written at or after `since` (ms): a
 * package left there by an earlier run, of this version or another, is
 * neither rewritten nor signed with this version.
 */
export function currentPackages(dir, suffix, version, since) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((name) => name.endsWith(suffix))
    .filter((name) => [`_${version}_`, `-${version}-`].some((part) => name.includes(part)))
    .map((name) => join(dir, name))
    .filter((file) => statSync(file).mtimeMs >= since);
}

/**
 * The key arguments and environment of `tauri signer sign`. The bundler
 * reads TAURI_SIGNING_PRIVATE_KEY as a file when it names one; the signer
 * only takes the key itself there, so a path goes by --private-key-path.
 */
export function signerKey(env, exists = existsSync) {
  const key = env.TAURI_SIGNING_PRIVATE_KEY;
  if (!key || !exists(key)) return { args: [], env };
  const { TAURI_SIGNING_PRIVATE_KEY: _path, ...rest } = env;
  return { args: ["--private-key-path", key], env: rest };
}
