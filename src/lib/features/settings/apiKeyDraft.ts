/**
 * The API key field after a successful save.
 *
 * Clear it only when it still holds the key that was just written. A different
 * value typed while `setApiKey` was in flight stays dirty, so the next save
 * writes that replacement instead of dropping it.
 */
export function apiKeyDraftAfterSuccessfulSave(
  current: string,
  saved: string,
): { apiKey: string; touched: boolean } {
  if (current.trim() === saved) {
    return { apiKey: "", touched: false };
  }
  return { apiKey: current, touched: true };
}
