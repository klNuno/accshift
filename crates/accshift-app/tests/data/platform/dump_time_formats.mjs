// Dumps what Intl.DateTimeFormat prints for the shipped locales, in UTC, to
// time_formats.json next to this file. Run: TZ=UTC node dump_time_formats.mjs
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const LOCALES = ["en", "es", "fr", "pt", "pt-br", "ru", "zh"];
// One stamp per month of 2025 (midday and just past midnight), a leap day,
// a year-end night and a far past date.
const STAMPS = [];
for (let month = 0; month < 12; month++) {
  STAMPS.push(Date.UTC(2025, month, 9, 7, 33, 20) / 1000);
  STAMPS.push(Date.UTC(2025, month, 28, 0, 5, 0) / 1000);
}
STAMPS.push(Date.UTC(2024, 1, 29, 12, 0, 0) / 1000);
STAMPS.push(Date.UTC(2023, 11, 31, 23, 59, 59) / 1000);
STAMPS.push(Date.UTC(1999, 0, 1, 13, 7, 0) / 1000);
STAMPS.push(1_760_000_000, 1_791_000_000);

const date = { year: "numeric", month: "short", day: "numeric", timeZone: "UTC" };
const dateTime = { ...date, hour: "2-digit", minute: "2-digit" };
const rows = [];
for (const locale of LOCALES) {
  for (const stamp of STAMPS) {
    const at = new Date(stamp * 1000);
    rows.push({
      locale,
      stamp,
      date: new Intl.DateTimeFormat(locale, date).format(at),
      dateTime: new Intl.DateTimeFormat(locale, dateTime).format(at),
    });
  }
}
const here = path.dirname(fileURLToPath(import.meta.url));
fs.writeFileSync(
  path.join(here, "time_formats.json"),
  JSON.stringify({ node: process.version, icu: process.versions.icu, rows }, null, 2) + "\n",
);
console.log(`rows: ${rows.length}`);
