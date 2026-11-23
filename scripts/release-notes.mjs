// Release notes in Boite's layout: a download per OS, then every commit of the
// release, grouped by conventional type, linked and credited.
//
//   node scripts/release-notes.mjs <releases.json> <out-file> <assets-dir>
//
// RELEASE_TAG and GITHUB_REPOSITORY are required. releases.json is the output
// of `gh api --paginate --slurp repos/<owner>/<repo>/releases`: the previous
// release is the last one published that is an ancestor of HEAD.
//
// A commit reverted inside the range never shipped: it leaves with its revert.
// When that revert is a squash that also carried other work, its
// "* feat|fix|perf|refactor: subject" lines are listed instead.
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export function previousRelease(releases, current, ancestor) {
  return releases
    .filter((release) => !release.draft && !release.prerelease && release.tag_name !== current)
    .filter((release) => /^v\d+\.\d+\.\d+$/.test(release.tag_name))
    .sort((a, b) => (b.published_at ?? "").localeCompare(a.published_at ?? ""))
    .find((release) => ancestor(release.tag_name))?.tag_name;
}

function escapeMarkdown(text) {
  return text.replace(/[\\`*_[\]<>]/g, "\\$&");
}

const SECTIONS = [
  ["feat", "Features"],
  ["fix", "Fixes"],
  ["perf", "Performance"],
];

// A GitHub noreply address names the account: `@login` links its profile and
// lists it among the release's contributors. Any other address shows the name.
function credit(author) {
  const match = /^(.*?)\s*<([^>]*)>$/.exec(author.trim());
  const name = match ? match[1] : author.trim();
  const login = /^(?:\d+\+)?([\w-]+)@users\.noreply\.github\.com$/i.exec(match?.[2] ?? "")?.[1];
  return login ? `@${login}` : escapeMarkdown(name);
}

function credits(authors = []) {
  const names = [...new Set(authors.map(credit).filter(Boolean))];
  return names.length > 0 ? ` by ${names.join(", ")}` : "";
}

function commitType(subject) {
  return /^(\w+)(?:\([^)]*\))?!?: /.exec(subject)?.[1];
}

// Only a whole line, as git revert writes it, with an optional PR number.
const REVERTS = /^\s*This reverts commit ([0-9a-f]{7,40})(?: \(#\d+\))?\.?\s*$/gim;
const SQUASHED = /^\* ((?:feat|fix|perf|refactor)(?:\([^)]*\))?!?: .+)$/;

/** Commits newest first, as git log lists them. Returns the entries to list. */
export function dropReverted(commits) {
  const cancelled = new Set();
  const entries = new Map();
  for (const commit of commits) {
    if (cancelled.has(commit)) continue;
    const prefixes = [...(commit.body ?? "").matchAll(REVERTS)].map((match) => match[1].toLowerCase());
    const targets = prefixes
      .map((prefix) => commits.find((other) => other !== commit && other.sha.toLowerCase().startsWith(prefix)))
      .filter(Boolean);
    if (targets.length === 0) continue;
    for (const target of targets) cancelled.add(target);
    // A revert of a change users already got stays: they saw it go.
    const own = targets.length < prefixes.length ? [commit] : [];
    const pr = / \(#\d+\)$/.exec(commit.subject)?.[0] ?? "";
    const squashed = (commit.body ?? "")
      .split(/\r?\n/)
      .map((line) => SQUASHED.exec(line.trim())?.[1])
      .filter(Boolean)
      .map((subject) => ({ ...commit, subject: subject + pr }));
    entries.set(commit, [...own, ...squashed]);
  }
  return commits.filter((commit) => !cancelled.has(commit)).flatMap((commit) => entries.get(commit) ?? [commit]);
}

const MACOS_NOTE =
  "macOS: the app is not notarized yet. After copying Accshift.app to Applications, run `xattr -cr /Applications/Accshift.app` once.";
const STREAMDECK_NOTE =
  "Stream Deck: open the plugin with Stream Deck 7.1 or later, then turn on Settings > General > Integrations > Allow the accshift CLI in accshift.";

/** Only manual downloads belong here; signatures and updater payloads stay in Assets. */
function downloadNotes(repository, tag, files) {
  const base = `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}`;
  const link = (label, name) => (files.includes(name) ? `[${label}](${base}/${encodeURIComponent(name)})` : "");
  const rows = [
    ["Windows (x64)", [link("Installer (.exe)", "accshift-windows-x64-setup.exe")]],
    ["macOS (Apple Silicon)", [link("Disk image (.dmg)", "accshift-macos-arm64.dmg")]],
    [
      "Linux (x64)",
      [
        link("Debian / Ubuntu (.deb)", "accshift-linux-amd64.deb"),
        link("Fedora / openSUSE (.rpm)", "accshift-linux-x86_64.rpm"),
        link("AppImage", "accshift-linux-x86_64.AppImage"),
      ],
    ],
    ["Stream Deck (Windows)", [link("Plugin (.streamDeckPlugin)", "accshift-streamdeck.streamDeckPlugin")]],
    [
      "Command line",
      [
        link("Windows", "accshift-cli-windows-x64.exe"),
        link("macOS", "accshift-cli-macos-arm64"),
        link("Linux", "accshift-cli-linux-x86_64"),
      ],
    ],
  ].filter(([, links]) => links.some(Boolean));
  if (rows.length === 0) return "";
  const notes = [
    files.includes("accshift-macos-arm64.dmg") ? MACOS_NOTE : "",
    files.includes("accshift-streamdeck.streamDeckPlugin") ? STREAMDECK_NOTE : "",
  ].filter(Boolean);
  return [
    "## Downloads",
    [
      "| Platform | Download |",
      "| --- | --- |",
      ...rows.map(([platform, links]) => `| ${platform} | ${links.filter(Boolean).join(" · ")} |`),
    ].join("\n"),
    notes.map((note) => `- ${note}`).join("\n"),
  ]
    .filter(Boolean)
    .join("\n\n");
}

export function releaseNotes(commits, repository, tag, previous, files = []) {
  const url = `https://github.com/${repository}`;
  const entry = ({ sha, subject, authors }) =>
    `- ${escapeMarkdown(subject)} ([${sha.slice(0, 7)}](${url}/commit/${sha}))${credits(authors)}`;
  const listed = dropReverted(commits);
  const known = new Set(SECTIONS.map(([type]) => type));
  const groups = [
    ...SECTIONS.map(([type, title]) => [title, listed.filter((commit) => commitType(commit.subject) === type)]),
    ["Other changes", listed.filter((commit) => !known.has(commitType(commit.subject) ?? ""))],
  ].filter(([, group]) => group.length > 0);
  const changes = groups.map(([title, group]) => `### ${title}\n\n${group.map(entry).join("\n")}`).join("\n\n");
  const compare = previous
    ? `[Full diff](${url}/compare/${encodeURIComponent(previous)}...${encodeURIComponent(tag)})`
    : "";
  const downloads = downloadNotes(repository, tag, files);
  return `${[downloads, "## What's changed", changes || "No code changes.", compare].filter(Boolean).join("\n\n")}\n`;
}

function git(args) {
  return execFileSync("git", args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
}

function main([releasesFile, out, assets]) {
  const tag = process.env.RELEASE_TAG;
  const repository = process.env.GITHUB_REPOSITORY;
  if (!releasesFile || !out || !assets) {
    throw new Error("usage: node scripts/release-notes.mjs <releases.json> <out-file> <assets-dir>");
  }
  if (!tag || !/^[\w.-]+\/[\w.-]+$/.test(repository ?? "")) {
    throw new Error("RELEASE_TAG and GITHUB_REPOSITORY (owner/repository) are required");
  }
  const releases = JSON.parse(readFileSync(releasesFile, "utf8")).flat();
  const previous = previousRelease(releases, tag, (candidate) => {
    try {
      execFileSync("git", ["merge-base", "--is-ancestor", `refs/tags/${candidate}`, "HEAD"], { stdio: "pipe" });
      return true;
    } catch (error) {
      if (error.status === 1) return false;
      throw new Error(`cannot resolve published release ${candidate}: ${error.stderr}`);
    }
  });
  const range = previous ? `refs/tags/${previous}..HEAD` : "HEAD";
  // One NUL-ended record per commit, fields split by US, co-authors by RS.
  const commits = git([
    "log",
    "--no-merges",
    "-z",
    "--format=%H%x1f%s%x1f%an <%ae>%x1f%(trailers:key=Co-authored-by,valueonly,separator=%x1e)%x1f%b",
    range,
  ])
    .split("\0")
    .filter((record) => record.trim() !== "")
    .map((record) => {
      const [sha = "", subject = "", author = "", coauthors = "", body = ""] = record.replace(/^\n/, "").split("\x1f");
      const authors = [author, ...coauthors.split("\x1e")].map((name) => name.trim()).filter(Boolean);
      return { sha: sha.trim(), subject: subject.trim(), authors, body };
    });
  const files = readdirSync(assets, { withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name);
  writeFileSync(out, releaseNotes(commits, repository, tag, previous, files));
  console.log(`Changelog: ${commits.length} commits since ${previous ?? "the first commit"}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main(process.argv.slice(2));
