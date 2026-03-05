function hashSeed(seed: string): number {
  let hash = 0;
  for (let i = 0; i < seed.length; i += 1) {
    hash = ((hash << 5) - hash) + seed.charCodeAt(i);
    hash |= 0;
  }
  return Math.abs(hash);
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

export function getAvatarGradientStyle(seed: string): string {
  const hash = hashSeed(seed);
  const hueA = hash % 360;
  const hueB = (hueA + 38 + (hash % 47)) % 360;
  const hueC = (hueB + 28 + (hash % 31)) % 360;

  return [
    `background-color:hsl(${hueA} 56% 44%)`,
    `background-image:linear-gradient(145deg,hsl(${hueA} 74% 58%),hsl(${hueB} 68% 44%) 58%,hsl(${hueC} 72% 34%))`,
  ].join(";");
}
