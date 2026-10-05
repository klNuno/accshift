import { describe, expect, it, vi } from "vitest";
import { completeOnboardingConsent } from "./onboardingConsent";

describe("completeOnboardingConsent", () => {
  it("returns saved when the choice is persisted", async () => {
    await expect(completeOnboardingConsent(async () => {})).resolves.toBe("saved");
  });

  it("returns failed when persistence rejects, without throwing", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(
      completeOnboardingConsent(async () => {
        throw new Error("disk");
      }),
    ).resolves.toBe("failed");
    error.mockRestore();
  });
});
