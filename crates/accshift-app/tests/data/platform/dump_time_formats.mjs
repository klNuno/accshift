// Dumps what Intl.DateTimeFormat prints for the shipped locales in UTC and in
// three named zones, to time_formats.json next to this file.
// Run: node dump_time_formats.mjs
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const LOCALES = ["en", "es", "fr", "pt", "pt-br", "ru", "zh"];
// Paris and New York have DST; Kolkata is a half-hour offset without DST
// since 1945 (before the Unix epoch, so outside what the formatters accept).
const ZONES = ["UTC", "Europe/Paris", "America/New_York", "Asia/Kolkata"];

// One stamp per month of 2025 (midday and just past midnight), a leap day,
// a year-end night and a far past date.
const BASE = [];
for (let month = 0; month < 12; month++) {
  BASE.push(Date.UTC(2025, month, 9, 7, 33, 20) / 1000);
  BASE.push(Date.UTC(2025, month, 28, 0, 5, 0) / 1000);
}
BASE.push(Date.UTC(2024, 1, 29, 12, 0, 0) / 1000);
BASE.push(Date.UTC(2023, 11, 31, 23, 59, 59) / 1000);
BASE.push(Date.UTC(1999, 0, 1, 13, 7, 0) / 1000);
BASE.push(1_760_000_000, 1_791_000_000);

// "+01:00"-style offset of `zone` at `stamp` seconds, in minutes.
const offsetFormats = new Map();
function offsetMinutes(zone, stamp) {
  if (!offsetFormats.has(zone)) {
    offsetFormats.set(
      zone,
      new Intl.DateTimeFormat("en", { timeZone: zone, timeZoneName: "longOffset" }),
    );
  }
  const name = offsetFormats
    .get(zone)
    .formatToParts(new Date(stamp * 1000))
    .find((part) => part.type === "timeZoneName").value;
  const match = /GMT(?:([+-])(\d{2}):(\d{2}))?/.exec(name);
  if (!match[1]) return 0;
  const minutes = Number(match[2]) * 60 + Number(match[3]);
  return match[1] === "-" ? -minutes : minutes;
}

// Every offset change of `zone` in `year`, found hour by hour then narrowed
// to the second.
function transitions(zone, year) {
  const found = [];
  const end = Date.UTC(year + 1, 0, 1) / 1000;
  for (let at = Date.UTC(year, 0, 1) / 1000; at < end; at += 3600) {
    if (offsetMinutes(zone, at) === offsetMinutes(zone, at + 3600)) continue;
    let low = at;
    let high = at + 3600;
    while (high - low > 1) {
      const mid = Math.floor((low + high) / 2);
      if (offsetMinutes(zone, mid) === offsetMinutes(zone, at)) low = mid;
      else high = mid;
    }
    found.push(high);
  }
  return found;
}

const rows = [];
const changes = {};
for (const zone of ZONES) {
  const stamps = [...BASE];
  // Both sides of each DST change: this year, next year, and 2040, past the
  // explicit tzdb transitions, where the POSIX rule takes over.
  changes[zone] = [2025, 2026, 2040].flatMap((year) => transitions(zone, year));
  for (const change of changes[zone]) {
    stamps.push(change - 3600, change - 1, change, change + 1800, change + 3600);
  }
  const date = { year: "numeric", month: "short", day: "numeric", timeZone: zone };
  const dateTime = { ...date, hour: "2-digit", minute: "2-digit" };
  for (const locale of LOCALES) {
    const dateFormat = new Intl.DateTimeFormat(locale, date);
    const dateTimeFormat = new Intl.DateTimeFormat(locale, dateTime);
    for (const stamp of stamps) {
      const at = new Date(stamp * 1000);
      rows.push({
        zone,
        locale,
        stamp,
        offset: offsetMinutes(zone, stamp),
        date: dateFormat.format(at),
        dateTime: dateTimeFormat.format(at),
      });
    }
  }
}
const here = path.dirname(fileURLToPath(import.meta.url));
fs.writeFileSync(
  path.join(here, "time_formats.json"),
  JSON.stringify(
    { node: process.version, icu: process.versions.icu, tz: process.versions.tz, changes, rows },
    null,
    2,
  ) + "\n",
);
console.log(`rows: ${rows.length}`);
for (const zone of ZONES) console.log(`${zone}: ${changes[zone].length} DST changes`);
