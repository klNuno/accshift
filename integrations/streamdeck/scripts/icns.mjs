// Reads one image out of an Apple .icns file (the app's icon.icns), for the
// plugin icon's @2x variant.

/**
 * The payload of the first entry of `type`, e.g. "ic09" (a 512x512 PNG).
 * @param {Buffer} icns
 * @param {string} type
 * @returns {Buffer}
 */
export function icnsPng(icns, type) {
  for (let at = 8; at + 8 <= icns.length; ) {
    const length = icns.readUInt32BE(at + 4);
    if (length < 8) break;
    if (icns.toString("latin1", at, at + 4) === type) return icns.subarray(at + 8, at + length);
    at += length;
  }
  throw new Error(`no ${type} image in the .icns`);
}
