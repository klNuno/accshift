import { describe, it, expect } from "vitest";
import { parseDeepLink } from "./deepLinkUrl";

describe("parseDeepLink", () => {
  it("parses a switch link", () => {
    expect(parseDeepLink("accshift://switch/steam/76561198000000001")).toEqual({
      action: "switch",
      platformId: "steam",
      accountRef: "76561198000000001",
    });
  });

  it("is case-insensitive on the scheme, action and platform", () => {
    expect(parseDeepLink("ACCSHIFT://Switch/Steam/bob")).toEqual({
      action: "switch",
      platformId: "steam",
      accountRef: "bob",
    });
  });

  it("keeps the account ref case as given", () => {
    expect(parseDeepLink("accshift://switch/steam/BobTheBuilder")?.accountRef).toBe(
      "BobTheBuilder",
    );
  });

  it("decodes percent-encoded segments", () => {
    expect(parseDeepLink("accshift://switch/riot/main%20smurf")?.accountRef).toBe("main smurf");
    expect(parseDeepLink("accshift://switch/battle-net/user%40example.com")?.accountRef).toBe(
      "user@example.com",
    );
  });

  it("tolerates trailing slash, query and fragment", () => {
    expect(parseDeepLink("accshift://switch/steam/bob/")?.accountRef).toBe("bob");
    expect(parseDeepLink("accshift://switch/steam/bob?source=streamdeck")?.accountRef).toBe("bob");
    expect(parseDeepLink("accshift://switch/steam/bob#x")?.accountRef).toBe("bob");
  });

  it("survives invalid percent-encoding", () => {
    expect(parseDeepLink("accshift://switch/steam/100%")?.accountRef).toBe("100%");
  });

  it("rejects other schemes", () => {
    expect(parseDeepLink("https://switch/steam/bob")).toBeNull();
    expect(parseDeepLink("steam://switch/steam/bob")).toBeNull();
  });

  it("rejects unknown actions", () => {
    expect(parseDeepLink("accshift://launch/steam/bob")).toBeNull();
  });

  it("rejects missing or extra segments", () => {
    expect(parseDeepLink("accshift://switch")).toBeNull();
    expect(parseDeepLink("accshift://switch/steam")).toBeNull();
    expect(parseDeepLink("accshift://switch/steam/bob/extra")).toBeNull();
  });

  it("rejects empty and whitespace-only refs", () => {
    expect(parseDeepLink("accshift://switch/steam/%20")).toBeNull();
    expect(parseDeepLink("")).toBeNull();
  });
});
