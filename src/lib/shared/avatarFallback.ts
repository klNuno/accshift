function normalizeSeed(seed: string): string {
  return seed.normalize("NFKC").trim().toLowerCase().replace(/\s+/g, " ");
}

function reverseText(text: string): string {
  return Array.from(text).reverse().join("");
}

function bitCycleHash(text: string, mult: number, modulo: number, seed: number): number {
  let acc = seed % modulo;
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i);
    for (let b = 0; b < 16; b += 1) {
      const bit = (code >> b) & 1;
      acc = (acc * mult + bit + b) % modulo;
    }
  }
  return acc;
}

export function getAvatarInitials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);

  if (parts.length === 0) return "?";
  if (parts.length === 1) {
    return parts[0].slice(0, 1).toUpperCase();
  }
  return `${parts[0][0] ?? ""}${parts[1][0] ?? ""}`.toUpperCase();
}

export function getAvatarSeed(displayName: string, username: string, accountId: string): string {
  const primary = (displayName || username || "").trim();
  const stable = primary || "unknown";
  const reversed = reverseText(stable);
  return `${stable}::${accountId}::${reversed}`;
}

export function getAvatarGradientStyle(seed: string): string {
  const normalized = normalizeSeed(seed || "?");
  const base = bitCycleHash(normalized, 33, 997, 17);
  const fade = bitCycleHash(normalized, 29, 991, 53);
  const hue = (base * 47) % 360;
  const hueFade = (hue + ((fade * 61) % 181) + 37) % 360;

  return [
    `background-color:hsl(${hue} 72% 43%)`,
    `background-image:linear-gradient(145deg,hsl(${hue} 80% 56%),hsl(${hueFade} 66% 38%))`,
  ].join(";");
}
