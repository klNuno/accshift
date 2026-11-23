import assert from "node:assert/strict";
import { test } from "node:test";
import { DOWNLOADS } from "./release-assets.mjs";
import { previousRelease, releaseNotes } from "./release-notes.mjs";

const sha = (char) => char.repeat(40);
const commit = (char, subject, body = "", authors = []) => ({ sha: sha(char), subject, body, authors });
const release = (tag, date, prerelease = false, draft = false) => ({
  tag_name: tag,
  published_at: date,
  prerelease,
  draft,
});
const notesOf = (commits, files = []) => releaseNotes(commits, "example/accshift", "v2.0.1", "v2.0.0", files);
const changes = (notes) => notes.slice(notes.indexOf("## What's changed"));

test("the previous release is the last published one that is an ancestor", () => {
  const releases = [
    release("v1.0.0", "2026-01-01"),
    release("v1.1.0-beta.1", "2026-01-02", true),
    release("v1.1.0", "2026-01-03", false, true),
    release("v9.0.0", "2026-01-05"),
    release("v2.0.0", "2026-01-04"),
  ];
  assert.equal(previousRelease(releases, "v2.0.0", (tag) => tag !== "v9.0.0"), "v1.0.0");
  assert.equal(previousRelease(releases, "v2.0.1", (tag) => tag !== "v9.0.0"), "v2.0.0");
  assert.equal(previousRelease([], "v1.0.0", () => true), undefined);
});

test("commits are grouped by type, linked, credited, with the full diff", () => {
  const notes = notesOf([
    commit("a", "fix(egui): keep [the] overlay size (#11)", "", [
      "meetsu <96637888+klNuno@users.noreply.github.com>",
      "Chris <76012699+ChrisPlayer@users.noreply.github.com>",
    ]),
    commit("b", "feat: add the widget (#10)", "", ["Jane_Doe <jane@example.com>"]),
    commit("c", "chore(deps): bump url"),
    commit("d", "perf: start sooner"),
  ]);
  const features = notes.indexOf("### Features");
  const fixes = notes.indexOf("### Fixes");
  const perf = notes.indexOf("### Performance");
  const other = notes.indexOf("### Other changes");
  assert.ok(features >= 0 && features < fixes && fixes < perf && perf < other);
  assert.ok(
    notes.includes(
      `- fix(egui): keep \\[the\\] overlay size (#11) ([aaaaaaa](https://github.com/example/accshift/commit/${sha("a")})) by @klNuno, @ChrisPlayer\n`,
    ),
  );
  assert.ok(notes.includes(`/commit/${sha("b")})) by Jane\\_Doe\n`));
  assert.ok(notes.slice(other).includes("bump url"));
  assert.ok(notes.trimEnd().endsWith("[Full diff](https://github.com/example/accshift/compare/v2.0.0...v2.0.1)"));
  assert.ok(!notes.includes("## Downloads"));
});

test("a first release has no full diff and an empty range says so", () => {
  const notes = releaseNotes([], "example/accshift", "v1.0.0", undefined);
  assert.equal(notes, "## What's changed\n\nNo code changes.\n");
});

test("downloads link each OS before the changes, never a signature or the updater feed", () => {
  const stable = DOWNLOADS.map(([name]) => name);
  const files = [
    ...stable,
    "accshift-windows-x64-setup.exe.sig",
    "accshift-linux-x86_64.AppImage.sig",
    "Accshift.app.tar.gz",
    "Accshift.app.tar.gz.sig",
    "latest.json",
    "SHA256SUMS.txt",
  ];
  const notes = notesOf([commit("a", "fix: one")], files);
  assert.ok(notes.startsWith("## Downloads\n\n| Platform | Download |"));
  assert.ok(notes.indexOf("## Downloads") < notes.indexOf("## What's changed"));
  for (const platform of ["Windows (x64)", "macOS (Apple Silicon)", "Linux (x64)", "Stream Deck", "Command line"]) {
    assert.ok(notes.includes(`| ${platform}`), platform);
  }
  for (const name of stable) {
    assert.ok(notes.includes(`https://github.com/example/accshift/releases/download/v2.0.1/${name})`), name);
  }
  assert.equal(notes.match(/\/releases\/download\//g).length, stable.length);
  assert.ok(!/\.sig\b|app\.tar\.gz|latest\.json|SHA256SUMS/.test(notes));
  assert.ok(notes.includes("xattr -cr /Applications/Accshift.app"));
  assert.ok(notes.includes("Stream Deck 7.1"));
});

test("downloads never invent an absent asset", () => {
  const notes = notesOf([], ["accshift-windows-x64-setup.exe"]);
  assert.ok(notes.includes("| Windows (x64) |"));
  for (const absent of ["macOS", "Linux", ".deb", "Stream Deck", "Command line", "xattr"]) {
    assert.ok(!notes.includes(absent), absent);
  }
});

test("a commit reverted in the range leaves with its revert, squashed fixes stay", () => {
  const notes = changes(
    notesOf([
      commit(
        "c",
        "revert: remove the widget (#12)",
        [
          "* revert: remove the widget",
          "",
          "This reverts commit aaaaaaa (#10).",
          "",
          "* docs(egui): list the backup card",
          "",
          "* fix(core): keep the legacy config",
          "",
          "* perf: cache the list",
        ].join("\n"),
      ),
      commit("b", "fix(egui): keep the overlay size (#11)"),
      commit("a", "feat: add the widget (#10)"),
    ]),
  );
  assert.doesNotMatch(notes, /widget|backup card/);
  assert.ok(notes.includes(`- fix(core): keep the legacy config (#12) ([ccccccc](`));
  assert.ok(notes.includes(`- perf: cache the list (#12) ([ccccccc](`));
  assert.ok(notes.includes("- fix(egui): keep the overlay size (#11)"));
});

test("a revert of a commit from an earlier release stays", () => {
  const notes = notesOf([commit("d", "revert: drop the old banner (#20)", "This reverts commit 0123456789.")]);
  assert.ok(notes.includes("- revert: drop the old banner (#20)"));
});

test("a revert of an in-range and a shipped commit keeps its own entry", () => {
  const notes = notesOf([
    commit(
      "b",
      "revert: drop the widget and the old banner (#51)",
      "This reverts commit aaaaaaaaaa (#50).\nThis reverts commit 0123456789.",
    ),
    commit("a", "feat: add the widget (#50)"),
  ]);
  assert.ok(notes.includes("- revert: drop the widget and the old banner (#51)"));
  assert.ok(!notes.includes("add the widget"));
});

test("a quoted revert line inside prose cancels nothing", () => {
  const notes = notesOf([
    commit("b", "docs: explain reverts (#41)", "Git writes `This reverts commit aaaaaaaaaa.` in the body."),
    commit("a", "feat: add the widget (#40)"),
  ]);
  assert.ok(notes.includes("add the widget (#40)"));
  assert.ok(notes.includes("explain reverts (#41)"));
});

test("a revert that is itself reverted cancels nothing", () => {
  const notes = notesOf([
    commit("c", 'Revert "Revert "feat: add the widget (#30)""', `This reverts commit ${sha("b")}.`),
    commit("b", 'Revert "feat: add the widget (#30)"', `This reverts commit ${sha("a")}.`),
    commit("a", "feat: add the widget (#30)"),
  ]);
  assert.ok(notes.includes("- feat: add the widget (#30)"));
  assert.ok(!notes.includes("Revert"));
});
