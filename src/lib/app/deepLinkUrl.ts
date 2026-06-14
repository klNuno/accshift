export interface DeepLinkSwitchRequest {
  action: "switch";
  platformId: string;
  accountRef: string;
}

/**
 * Parses an accshift:// URL. Supported form:
 *   accshift://switch/<platformId>/<accountRef>
 * accountRef matches the account id first, then username, then display name.
 */
export function parseDeepLink(rawUrl: string): DeepLinkSwitchRequest | null {
  const match = /^accshift:\/\/([^?#]*)/i.exec(rawUrl.trim());
  if (!match) return null;
  const segments = match[1]
    .split("/")
    .filter(Boolean)
    .map((segment) => {
      try {
        return decodeURIComponent(segment);
      } catch {
        return segment;
      }
    });
  if (segments.length === 3 && segments[0].toLowerCase() === "switch") {
    const platformId = segments[1].toLowerCase();
    const accountRef = segments[2].trim();
    if (platformId && accountRef) {
      return { action: "switch", platformId, accountRef };
    }
  }
  return null;
}
