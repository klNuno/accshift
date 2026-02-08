// Convert SteamID64 to Steam profile URL
export function toProfileUrl(steamId64: string): string {
  return `https://steamcommunity.com/profiles/${steamId64}`;
}
