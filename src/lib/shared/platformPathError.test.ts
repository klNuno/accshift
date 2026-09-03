import { describe, it, expect } from "vitest";
import { parsePlatformPathError, platformPathErrorMessage } from "./platformPathError";
import { EN_MESSAGES, type MessageKey } from "$lib/i18n/messages";

const t = (key: MessageKey) => EN_MESSAGES[key];
const GENERIC = "Couldn't save the Steam path";

describe("parsePlatformPathError", () => {
  it("splits a coded rejection into its code and English fallback", () => {
    expect(
      parsePlatformPathError(
        "steam_path_never_signed_in|Steam found, sign in once so it creates its login history",
      ),
    ).toEqual({
      code: "steam_path_never_signed_in",
      fallback: "Steam found, sign in once so it creates its login history",
    });
  });

  it("reports no code for a plain message", () => {
    expect(parsePlatformPathError("Could not locate Steam installation")).toEqual({
      code: null,
      fallback: "Could not locate Steam installation",
    });
  });

  // A pipe inside prose must not be read as a code separator, or an unrelated
  // error would lose its first words.
  it("reports no code when the left half is not one", () => {
    const message = "Could not write config | retry as administrator";
    expect(parsePlatformPathError(message)).toEqual({ code: null, fallback: message });
  });

  it("accepts a non-string rejection", () => {
    expect(parsePlatformPathError(new Error("steam_path_not_steam|nope")).code).toBe(null);
    expect(parsePlatformPathError(undefined)).toEqual({ code: null, fallback: "" });
  });
});

describe("platformPathErrorMessage", () => {
  it("translates the never-signed-in code", () => {
    const message = platformPathErrorMessage(
      "steam_path_never_signed_in|Steam found, sign in once so it creates its login history",
      t,
      GENERIC,
    );
    expect(message).toBe("Steam found, sign in once so it creates its login history");
  });

  it("translates the other picker codes", () => {
    expect(platformPathErrorMessage("steam_path_not_steam|whatever", t, GENERIC)).toBe(
      EN_MESSAGES["settings.pathNotSteamFolder"],
    );
    expect(platformPathErrorMessage("steam_path_not_a_directory|whatever", t, GENERIC)).toBe(
      EN_MESSAGES["settings.pathNotADirectory"],
    );
  });

  // An error the backend adds later must read as the generic line, never as a
  // raw code the user cannot act on.
  it("falls back to the generic message for an unknown code", () => {
    expect(platformPathErrorMessage("steam_path_from_the_future|nope", t, GENERIC)).toBe(GENERIC);
  });

  it("falls back to the generic message for an uncoded failure", () => {
    expect(platformPathErrorMessage("disk full", t, GENERIC)).toBe(GENERIC);
  });
});
