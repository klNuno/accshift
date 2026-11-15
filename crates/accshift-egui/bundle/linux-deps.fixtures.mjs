// Small deb and rpm files for linux-deps.test.mjs, written and read here
// without the code under test, in the layouts the Tauri bundler produces.
import { createHash } from "node:crypto";
import { gunzipSync, gzipSync } from "node:zlib";

// --- deb

function arMember(name, data) {
  const header = Buffer.alloc(60, 0x20);
  header.write(`${name}/`, 0, "latin1");
  header.write("0", 16, "latin1");
  header.write("0", 28, "latin1");
  header.write("0", 34, "latin1");
  header.write("100644", 40, "latin1");
  header.write(String(data.length), 48, "latin1");
  header.write("`\n", 58, "latin1");
  return Buffer.concat([header, data, data.length % 2 ? Buffer.from("\n") : Buffer.alloc(0)]);
}

function tarEntry(name, content) {
  const header = Buffer.alloc(512);
  header.write(name, 0, "latin1");
  header.write("0000644\0", 100, "latin1");
  header.write("0000000\0", 108, "latin1");
  header.write("0000000\0", 116, "latin1");
  header.write(`${content.length.toString(8).padStart(11, "0")}\0`, 124, "latin1");
  header.write("00000000000\0", 136, "latin1");
  header.write("0", 156, "latin1");
  header.write("ustar\0", 257, "latin1");
  header.write("00", 263, "latin1");
  header.fill(0x20, 148, 156);
  const sum = header.reduce((total, byte) => total + byte, 0);
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, "latin1");
  const data = Buffer.alloc(Math.ceil(content.length / 512) * 512);
  content.copy(data);
  return Buffer.concat([header, data]);
}

/** A deb whose control file is `control`, with a data member of `payload`. */
export function makeDeb(control, payload) {
  const controlTar = Buffer.concat([
    tarEntry("./control", Buffer.from(control, "utf8")),
    tarEntry("./md5sums", Buffer.from("d41d8cd98f00b204e9800998ecf8427e  usr/bin/accshift-gui\n")),
    Buffer.alloc(1024),
  ]);
  return Buffer.concat([
    Buffer.from("!<arch>\n", "latin1"),
    arMember("debian-binary", Buffer.from("2.0\n")),
    arMember("control.tar.gz", gzipSync(controlTar)),
    arMember("data.tar.gz", payload),
  ]);
}

/** The members of a deb: name to bytes. */
export function readDeb(buffer) {
  assert(buffer.toString("latin1", 0, 8) === "!<arch>\n", "not an ar archive");
  const members = {};
  let at = 8;
  while (at < buffer.length) {
    const name = buffer.toString("latin1", at, at + 16).trim().replace(/\/$/, "");
    const size = Number.parseInt(buffer.toString("latin1", at + 48, at + 58).trim(), 10);
    members[name] = buffer.subarray(at + 60, at + 60 + size);
    at += 60 + size + (size % 2);
  }
  return members;
}

/** The files of the deb's control archive, with a checksum check per entry. */
export function readControlTar(member) {
  const tar = gunzipSync(member);
  const files = {};
  let at = 0;
  while (at + 512 <= tar.length && tar[at] !== 0) {
    const header = Buffer.from(tar.subarray(at, at + 512));
    const name = header.toString("latin1", 0, 100).replace(/\0.*$/s, "");
    const size = Number.parseInt(header.toString("latin1", 124, 136), 8);
    const stored = Number.parseInt(header.toString("latin1", 148, 156), 8);
    header.fill(0x20, 148, 156);
    assert(stored === header.reduce((total, byte) => total + byte, 0), `bad tar checksum for ${name}`);
    files[name] = tar.toString("utf8", at + 512, at + 512 + size);
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return files;
}

// --- rpm

const MAGIC = Buffer.from([0x8e, 0xad, 0xe8, 0x01, 0, 0, 0, 0]);
export const INT32 = 4;
export const STRING = 6;
export const BIN = 7;
export const STRING_ARRAY = 8;
const ALIGN = { 3: 2, 4: 4, 5: 8 };

function encode(type, value) {
  if (type === INT32) {
    const data = Buffer.alloc(value.length * 4);
    value.forEach((v, i) => data.writeUInt32BE(v, i * 4));
    return { data, count: value.length };
  }
  if (type === STRING) return { data: Buffer.from(`${value}\0`, "utf8"), count: 1 };
  if (type === STRING_ARRAY) return { data: Buffer.from(value.map((v) => `${v}\0`).join(""), "utf8"), count: value.length };
  if (type === BIN) return { data: value, count: value.length };
  throw new Error(`type ${type}`);
}

/** A header from `[tag, type, value]` entries, data in entry order. */
export function header(entries) {
  const index = [];
  const store = [];
  let size = 0;
  for (const [tag, type, value] of entries) {
    const pad = ((ALIGN[type] ?? 1) - (size % (ALIGN[type] ?? 1))) % (ALIGN[type] ?? 1);
    if (pad) store.push(Buffer.alloc(pad));
    size += pad;
    const { data, count } = encode(type, value);
    index.push({ tag, type, offset: size, count });
    store.push(data);
    size += data.length;
  }
  const head = Buffer.alloc(16 + index.length * 16);
  MAGIC.copy(head);
  head.writeUInt32BE(index.length, 8);
  head.writeUInt32BE(size, 12);
  index.forEach(({ tag, type, offset, count }, i) => {
    head.writeUInt32BE(tag, 16 + i * 16);
    head.writeUInt32BE(type, 20 + i * 16);
    head.writeInt32BE(offset, 24 + i * 16);
    head.writeUInt32BE(count, 28 + i * 16);
  });
  return Buffer.concat([head, ...store]);
}

const hex = (algorithm, data) => createHash(algorithm).update(data).digest("hex");

/**
 * An rpm of `main` (a header) and `payload`, its signature header holding
 * the digests the bundler writes, plus `extraSig` entries.
 */
export function makeRpm(main, payload, extraSig = []) {
  const lead = Buffer.alloc(96);
  lead.writeUInt32BE(0xedabeedb, 0);
  const sig = header([
    [1000, INT32, [main.length + payload.length]],
    [269, STRING, hex("sha1", main)],
    [273, STRING, hex("sha256", main)],
    [1004, BIN, createHash("md5").update(main).update(payload).digest()],
    ...extraSig,
  ]);
  const pad = Buffer.alloc((8 - (sig.length % 8)) % 8);
  return Buffer.concat([lead, sig, pad, main, payload]);
}

function parseHeader(buffer, at) {
  assert(buffer.subarray(at, at + 8).equals(MAGIC), `no header at ${at}`);
  const count = buffer.readUInt32BE(at + 8);
  const size = buffer.readUInt32BE(at + 12);
  const store = buffer.subarray(at + 16 + count * 16, at + 16 + count * 16 + size);
  const tags = {};
  for (let i = 0; i < count; i += 1) {
    const e = at + 16 + i * 16;
    const tag = buffer.readUInt32BE(e);
    const type = buffer.readUInt32BE(e + 4);
    const offset = buffer.readInt32BE(e + 8);
    const n = buffer.readUInt32BE(e + 12);
    if (type === INT32) {
      assert(offset % 4 === 0, `tag ${tag} misaligned`);
      tags[tag] = Array.from({ length: n }, (_, k) => store.readUInt32BE(offset + k * 4));
    } else if (type === STRING || type === STRING_ARRAY) {
      const values = [];
      let p = offset;
      for (let k = 0; k < n; k += 1) {
        const end = store.indexOf(0, p);
        values.push(store.toString("utf8", p, end));
        p = end + 1;
      }
      tags[tag] = type === STRING ? values[0] : values;
    } else if (type === BIN) {
      tags[tag] = Buffer.from(store.subarray(offset, offset + n));
    }
  }
  return { tags, end: at + 16 + count * 16 + size };
}

/**
 * The rpm's signature and main tags, and whether its digests and size
 * match the bytes, as `rpm -K` checks them.
 */
export function readRpm(buffer) {
  const sig = parseHeader(buffer, 96);
  const mainAt = sig.end + ((8 - (sig.end % 8)) % 8);
  const main = parseHeader(buffer, mainAt);
  const headerBytes = buffer.subarray(mainAt, main.end);
  const payload = buffer.subarray(main.end);
  const checks = {
    size: sig.tags[1000][0] === headerBytes.length + payload.length,
    sha1: sig.tags[269] === hex("sha1", headerBytes),
    sha256: sig.tags[273] === hex("sha256", headerBytes),
    md5: sig.tags[1004].equals(createHash("md5").update(headerBytes).update(payload).digest()),
  };
  return { sig: sig.tags, main: main.tags, checks, payload: Buffer.from(payload), sigBytes: buffer.subarray(0, sig.end) };
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
