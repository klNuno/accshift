/** Persists the onboarding choice. A rejection stays in the dialog: the
 *  backdrop covers the toast host, so the caller shows the failure inline. */
export async function completeOnboardingConsent(
  persist: () => Promise<unknown>,
): Promise<"saved" | "failed"> {
  try {
    await persist();
    return "saved";
  } catch (error) {
    console.error("telemetry_complete_onboarding failed", error);
    return "failed";
  }
}
