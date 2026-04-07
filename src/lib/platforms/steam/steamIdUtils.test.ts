import { describe, it, expect } from "vitest";
import { toProfileUrl } from "./steamIdUtils";

describe("toProfileUrl", () => {
  it("builds correct Steam community URL", () => {
    expect(toProfileUrl("76561198123456789")).toBe(
      "https://steamcommunity.com/profiles/76561198123456789",
    );
  });

  it("works with any string", () => {
    expect(toProfileUrl("12345")).toBe("https://steamcommunity.com/profiles/12345");
  });
});
