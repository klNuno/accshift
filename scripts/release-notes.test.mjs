import assert from "node:assert/strict";
import { test } from "node:test";
import { buildNotes, parseLog } from "./release-notes.mjs";

const commit = (hash, subject, body = "") => ({ hash, subject, body });

test("groups subjects into changes and fixes, types stripped, dash lines as details", () => {
  const notes = buildNotes([
    commit("a1", "feat(core): add the widget (#1)", "- first detail\n- second detail\n* ignored"),
    commit("b2", "fix: stop the crash (#2)"),
    commit("c3", "perf(egui): start sooner (#3)"),
  ]);
  assert.equal(
    notes,
    [
      "## Changes",
      "",
      "- add the widget (#1): first detail; second detail",
      "- start sooner (#3)",
      "",
      "## Fixes",
      "",
      "- stop the crash (#2)",
    ].join("\n"),
  );
});

test("a wrapped detail keeps its indented lines", () => {
  const notes = buildNotes([
    commit(
      "a1",
      "fix: guard the pipe (#4)",
      "- egui: a handler could load the fd just\n  before teardown closed it.\n- cli: labels match\nNot a continuation.",
    ),
  ]);
  assert.equal(
    notes,
    "## Fixes\n\n- guard the pipe (#4): egui: a handler could load the fd just before teardown closed it.; cli: labels match",
  );
});

test("a commit reverted in the range leaves with its revert, squashed fixes stay", () => {
  const notes = buildNotes([
    commit("aaaaaaa1111", "feat: add the widget (#10)"),
    commit("bbbbbbb2222", "fix(egui): keep the overlay size (#11)"),
    commit(
      "ccccccc3333",
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
  ]);
  assert.doesNotMatch(notes, /widget/);
  assert.doesNotMatch(notes, /backup card/);
  assert.equal(
    notes,
    [
      "## Changes",
      "",
      "- cache the list (#12)",
      "",
      "## Fixes",
      "",
      "- keep the overlay size (#11)",
      "- keep the legacy config (#12)",
    ].join("\n"),
  );
});

test("a revert of a commit from an earlier release stays in the notes", () => {
  const notes = buildNotes([commit("ddd4444", "revert: drop the old banner (#20)", "This reverts commit 0123456789.")]);
  assert.equal(notes, "## Changes\n\n- drop the old banner (#20)");
});

test("a revert that is itself reverted cancels nothing", () => {
  const notes = buildNotes([
    commit("1111111aaa", "feat: add the widget (#30)"),
    commit("2222222bbb", 'Revert "feat: add the widget (#30)"', "This reverts commit 1111111aaa."),
    commit("3333333ccc", 'Revert "Revert "feat: add the widget (#30)""', "This reverts commit 2222222bbb."),
  ]);
  assert.equal(notes, "## Changes\n\n- add the widget (#30)");
});

test("an empty range gets the placeholder entry", () => {
  assert.equal(buildNotes([]), "## Changes\n\n- No changelog entries found.");
});

test("parseLog reads hash, subject and body records", () => {
  const raw = "h1\x1ffeat: one (#1)\x1fbody line\n\x1e\nh2\x1ffix: two (#2)\x1f\x1e";
  assert.deepEqual(parseLog(raw), [
    { hash: "h1", subject: "feat: one (#1)", body: "body line\n" },
    { hash: "h2", subject: "fix: two (#2)", body: "" },
  ]);
});
