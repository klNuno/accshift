import { describe, expect, it } from "vitest";

import { createBulkEditController } from "./useBulkEdit.svelte";

function controllerOver(visibleIds: () => string[]) {
  return createBulkEditController({
    getCurrentAccountId: () => null,
    getVisibleAccountIds: visibleIds,
    getBulkEditCapability: () => null,
  });
}

describe("bulk edit selection", () => {
  it("keeps what was picked elsewhere when selecting all inside a folder", () => {
    let visible = ["root-1", "root-2"];
    const bulk = controllerOver(() => visible);

    bulk.bulkEditSelectAll();
    expect([...bulk.bulkEditSelectedIds]).toEqual(["root-1", "root-2"]);

    // Navigating into a folder only changes what is on screen.
    visible = ["folder-1"];
    bulk.bulkEditSelectAll();

    expect([...bulk.bulkEditSelectedIds]).toEqual(["root-1", "root-2", "folder-1"]);
  });

  it("keeps a manual pick made in another folder", () => {
    let visible = ["root-1"];
    const bulk = controllerOver(() => visible);

    bulk.toggleBulkEditAccount("root-1");
    visible = ["folder-1"];
    bulk.toggleBulkEditAccount("folder-1");

    expect([...bulk.bulkEditSelectedIds]).toEqual(["root-1", "folder-1"]);
  });

  it("deselect all is the reset, across folders", () => {
    const bulk = controllerOver(() => ["root-1"]);

    bulk.bulkEditSelectAll();
    bulk.toggleBulkEditAccount("folder-1");
    bulk.bulkEditDeselectAll();

    expect(bulk.bulkEditSelectedIds.size).toBe(0);
  });
});
