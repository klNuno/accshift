import type { CachedPlatformProfile, PlatformProfileInfo } from "$lib/shared/platform";
import { getCachedRiotAccount } from "./accountCache";

function createAvatarSvg(accountId: string, displayName: string): string {
  const initials = displayName
    .split(/\s+/)
    .map((part) => part[0] ?? "")
    .join("")
    .slice(0, 2)
    .toUpperCase();
  const hue = Array.from(accountId).reduce((sum, char) => sum + char.charCodeAt(0), 0) % 360;
  const svg = `
    <svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">
      <defs>
        <linearGradient id="bg" x1="0" x2="1" y1="0" y2="1">
          <stop offset="0%" stop-color="hsl(${hue} 74% 52%)" />
          <stop offset="100%" stop-color="hsl(${(hue + 28) % 360} 82% 38%)" />
        </linearGradient>
      </defs>
      <rect width="128" height="128" rx="26" fill="url(#bg)" />
      <text x="64" y="74" text-anchor="middle" font-family="Segoe UI, Arial, sans-serif" font-size="40" font-weight="700" fill="white">${initials}</text>
    </svg>
  `.replace(/\s+/g, " ").trim();
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

export function getRiotProfile(accountId: string): PlatformProfileInfo | null {
  const account = getCachedRiotAccount(accountId);
  if (!account) return null;
  return {
    avatarUrl: createAvatarSvg(account.id, account.display_name),
    displayName: account.display_name,
  };
}

export function getCachedRiotProfile(accountId: string): CachedPlatformProfile | null {
  const profile = getRiotProfile(accountId);
  if (!profile?.avatarUrl) return null;
  return {
    url: profile.avatarUrl,
    displayName: profile.displayName ?? undefined,
    expired: false,
  };
}
