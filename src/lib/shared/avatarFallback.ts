function normalizeSeed(seed: string): string {
  return seed
    .normalize("NFKC")
    .trim()
    .toLowerCase()
    .replace(/\s+/g, " ");
}

function hashSeed(seed: string): number {
  // Simple, deterministic FNV-1a hash.
  let hash = 0x811c9dc5;
  for (let i = 0; i < seed.length; i += 1) {
    hash ^= seed.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

function mix32(hash: number): number {
  // Tiny avalanche step to decorrelate close seeds.
  let x = hash >>> 0;
  x ^= x >>> 16;
  x = Math.imul(x, 0x85ebca6b);
  x ^= x >>> 13;
  x = Math.imul(x, 0xc2b2ae35);
  x ^= x >>> 16;
  return x >>> 0;
}

export function getAvatarInitials(name: string): string {
  const parts = name
    .trim()
    .split(/\s+/)
    .filter(Boolean);

  if (parts.length === 0) return "?";
  if (parts.length === 1) {
    return parts[0].slice(0, 1).toUpperCase();
  }
  return `${parts[0][0] ?? ""}${parts[1][0] ?? ""}`.toUpperCase();
}

export function getAvatarSeed(displayName: string, username: string, accountId: string): string {
  const primary = (displayName || username || "").trim();
  if (!primary) return accountId;
  return `${primary}::${accountId}`;
}

export function getAvatarGradientStyle(seed: string): string {
  const normalized = normalizeSeed(seed || "?");
  const mixed = mix32(hashSeed(normalized));

  const hue = Math.floor((mixed / 0xffffffff) * 360);
  const hue2 = (hue + 137) % 360;
  const sat = 70 + ((mixed >>> 24) % 14); // 70..83
  const light = 45 + ((mixed >>> 16) % 10); // 45..54

  return [
    `background-color:hsl(${hue} ${sat}% ${Math.max(32, light - 8)}%)`,
    `background-image:linear-gradient(145deg,hsl(${hue} ${sat}% ${Math.min(66, light + 8)}%),hsl(${hue2} ${Math.max(55, sat - 8)}% ${Math.max(34, light - 6)}%))`,
  ].join(";");
}
