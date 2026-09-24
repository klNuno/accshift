import { describe, expect, it, vi } from "vitest";
import type { PlatformBulkEditCapability } from "$lib/shared/platform";

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

describe("bulk edit bar loading", () => {
  function deferredBar() {
    let resolve!: () => void;
    const loaded = new Promise<void>((done) => {
      resolve = done;
    });
    const Bar = (() => {}) as never;
    const capability: PlatformBulkEditCapability = {
      loadBar: vi.fn(async () => {
        await loaded;
        return { default: Bar };
      }),
    };
    return { capability, resolve, Bar };
  }

  function controllerWith(getCapability: () => PlatformBulkEditCapability | null) {
    return createBulkEditController({
      getCurrentAccountId: () => null,
      getVisibleAccountIds: () => [],
      getBulkEditCapability: getCapability,
    });
  }

  const settle = () => new Promise((done) => setTimeout(done, 0));

  it("opens once the bar has loaded", async () => {
    const steam = deferredBar();
    const bulk = controllerWith(() => steam.capability);

    bulk.toggleBulkEdit();
    steam.resolve();
    await settle();

    expect(bulk.bulkEditMode).toBe(true);
    expect(bulk.BulkEditBar).toBe(steam.Bar);
  });

  it("stays closed when the tab changed while the bar loaded", async () => {
    const steam = deferredBar();
    let capability: PlatformBulkEditCapability | null = steam.capability;
    const bulk = controllerWith(() => capability);

    bulk.toggleBulkEdit();
    capability = null;
    bulk.closeBulkEdit();
    steam.resolve();
    await settle();

    expect(bulk.bulkEditMode).toBe(false);
    expect(bulk.BulkEditBar).toBeNull();
  });

  it("treats a second toggle during the load as a cancel", async () => {
    const steam = deferredBar();
    const bulk = controllerWith(() => steam.capability);

    bulk.toggleBulkEdit();
    bulk.toggleBulkEdit();
    steam.resolve();
    await settle();

    expect(bulk.bulkEditMode).toBe(false);
  });

  it("opens on a third toggle that asks again before the load ends", async () => {
    const steam = deferredBar();
    const bulk = controllerWith(() => steam.capability);

    bulk.toggleBulkEdit();
    bulk.toggleBulkEdit();
    bulk.toggleBulkEdit();
    steam.resolve();
    await settle();

    expect(bulk.bulkEditMode).toBe(true);
  });
});
