import { describe, expect, it } from "vitest";
import { apiKeyDraftAfterSuccessfulSave } from "./apiKeyDraft";

describe("apiKeyDraftAfterSuccessfulSave", () => {
  it("clears the input when it still holds the key that was saved", () => {
    expect(apiKeyDraftAfterSuccessfulSave("  steam-key  ", "steam-key")).toEqual({
      apiKey: "",
      touched: false,
    });
  });

  it("keeps a replacement typed while the first save was in flight", () => {
    expect(apiKeyDraftAfterSuccessfulSave("next-key", "steam-key")).toEqual({
      apiKey: "next-key",
      touched: true,
    });
  });
});
