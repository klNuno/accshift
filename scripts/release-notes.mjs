// Builds the release notes from the commits of a release range.
//
//   node scripts/release-notes.mjs <range> [out-file]
//
// Each commit subject becomes one entry, its conventional type stripped, under
// "## Changes" or "## Fixes". Body lines starting with "- " become details.
//
// A commit reverted inside the range never shipped, so it leaves the notes
// together with its revert. When that revert is a squash that also carried
// other work, its "* type: subject" lines for feat, fix, perf and refactor are
// listed instead, with the squash's PR number. A revert of a commit from an
// earlier release stays: users saw that change go.
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const TYPE_PREFIX = /^(build|chore|ci|docs|feat|fix|perf|refactor|revert|style|test)(\([^)]+\))?!?:\s*/i;
const REVERTS = /This reverts commit ([0-9a-f]{7,40})/gi;
const SQUASHED = /^\* (feat|fix|perf|refactor)(\([^)]+\))?!?:\s*(.+)$/i;
const PR_NUMBER = /\s\(#\d+\)$/;

export function parseLog(raw) {
  return raw
    .split("\x1e")
    .filter((record) => record.trim() !== "")
    .map((record) => {
      const [hash = "", subject = "", body = ""] = record.split("\x1f");
      return { hash: hash.trim(), subject: subject.trim(), body };
    })
    .filter((commit) => commit.subject !== "");
}

function normalizeSubject(subject) {
  const normalized = subject.trim().replace(TYPE_PREFIX, "");
  return normalized.trim() === "" ? subject.trim() : normalized;
}

function sectionOf(subject, normalized) {
  if (/^(fix|hotfix)(\([^)]+\))?!?:/i.test(subject) || /^(fix|hotfix|bugfix)\b/i.test(normalized)) {
    return "Fixes";
  }
  return "Changes";
}

// A "- " line opens a detail; the indented lines under it wrap that detail.
function details(body) {
  const found = [];
  let open = false;
  for (const line of body.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed.startsWith("- ")) {
      found.push(trimmed.slice(2).trim());
      open = true;
    } else if (open && trimmed !== "" && /^\s/.test(line)) {
      found[found.length - 1] += ` ${trimmed}`;
    } else {
      open = false;
    }
  }
  return found;
}

function revertTargets(commit, commits) {
  const prefixes = [...commit.body.matchAll(REVERTS)].map((match) => match[1].toLowerCase());
  return commits.filter((other) => other !== commit && prefixes.some((p) => other.hash.toLowerCase().startsWith(p)));
}

// Walks newest first, so a revert that is itself reverted cancels nothing.
function cancelReverts(commits) {
  const cancelled = new Set();
  const revertsInRange = new Set();
  for (const commit of [...commits].reverse()) {
    if (cancelled.has(commit)) continue;
    const targets = revertTargets(commit, commits);
    if (targets.length === 0) continue;
    revertsInRange.add(commit);
    for (const target of targets) cancelled.add(target);
  }
  return { cancelled, revertsInRange };
}

function squashedEntries(commit) {
  const pr = commit.subject.match(PR_NUMBER)?.[0] ?? "";
  return commit.body
    .split(/\r?\n/)
    .map((line) => line.trim().match(SQUASHED))
    .filter(Boolean)
    .map((match) => {
      const subject = `${match[1]}${match[2] ?? ""}: ${match[3].trim()}`;
      const normalized = `${match[3].trim()}${pr}`;
      return { section: sectionOf(subject, normalized), subject: normalized, details: [] };
    });
}

export function buildNotes(commits) {
  const { cancelled, revertsInRange } = cancelReverts(commits);
  const grouped = { Changes: [], Fixes: [] };
  for (const commit of commits) {
    if (cancelled.has(commit)) continue;
    const entries = revertsInRange.has(commit)
      ? squashedEntries(commit)
      : [
          {
            section: sectionOf(commit.subject, normalizeSubject(commit.subject)),
            subject: normalizeSubject(commit.subject),
            details: details(commit.body),
          },
        ];
    for (const entry of entries) grouped[entry.section].push(entry);
  }

  const lines = [];
  for (const section of ["Changes", "Fixes"]) {
    if (grouped[section].length === 0) continue;
    lines.push(`## ${section}`, "");
    for (const entry of grouped[section]) {
      lines.push(entry.details.length > 0 ? `- ${entry.subject}: ${entry.details.join("; ")}` : `- ${entry.subject}`);
    }
    lines.push("");
  }
  const text = lines.join("\n").trim();
  return text === "" ? "## Changes\n\n- No changelog entries found." : text;
}

function main([range, out]) {
  if (!range) throw new Error("usage: node scripts/release-notes.mjs <range> [out-file]");
  const raw = execFileSync("git", ["log", "--reverse", "--pretty=format:%H%x1f%s%x1f%b%x1e", range], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  const notes = buildNotes(parseLog(raw));
  if (out) writeFileSync(out, `${notes}\n`);
  console.log(notes);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main(process.argv.slice(2));
