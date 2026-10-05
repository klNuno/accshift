import { describe, expect, it, vi } from "vitest";
import { persistThemeDraft } from "./themeSave";

describe("persistThemeDraft", () => {
  it("returns saved when the document write resolves", async () => {
    await expect(persistThemeDraft(async () => {})).resolves.toBe("saved");
  });

  it("returns failed when the document write rejects, without throwing", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(
      persistThemeDraft(async () => {
        throw new Error("disk");
      }),
    ).resolves.toBe("failed");
    error.mockRestore();
  });
});
