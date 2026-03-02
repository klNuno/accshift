import type { PlatformAccount, PlatformProfileInfo } from "$lib/shared/platform";

const STORAGE_KEY = "accshift_riot_mock_accounts";
const CURRENT_KEY = "accshift_riot_mock_current";

interface RiotMockAccount extends PlatformAccount {
  region: string;
  tagLine: string;
}

const DEFAULT_ACCOUNTS: RiotMockAccount[] = [
  {
    id: "riot-euw-jett-01",
    username: "JettMain#EUW",
    displayName: "Jett Main",
    region: "EUW",
    tagLine: "EUW",
    lastLoginAt: Date.now() - 1000 * 60 * 18,
  },
  {
    id: "riot-na-omen-02",
    username: "ShadowOmen#NA1",
    displayName: "Shadow Omen",
    region: "NA",
    tagLine: "NA1",
    lastLoginAt: Date.now() - 1000 * 60 * 60 * 7,
  },
  {
    id: "riot-apac-viper-03",
    username: "ViperLab#APC",
    displayName: "Viper Lab",
    region: "APAC",
    tagLine: "APC",
    lastLoginAt: Date.now() - 1000 * 60 * 60 * 26,
  },
];

function canUseStorage() {
  return typeof localStorage !== "undefined";
}

function readAccounts(): RiotMockAccount[] {
  if (!canUseStorage()) return DEFAULT_ACCOUNTS;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_ACCOUNTS;
    const parsed = JSON.parse(raw) as RiotMockAccount[];
    if (!Array.isArray(parsed) || parsed.length === 0) return DEFAULT_ACCOUNTS;
    return parsed.filter((account) =>
      typeof account?.id === "string"
      && typeof account?.username === "string"
      && typeof account?.displayName === "string"
      && typeof account?.region === "string"
      && typeof account?.tagLine === "string"
    );
  } catch {
    return DEFAULT_ACCOUNTS;
  }
}

function saveAccounts(accounts: RiotMockAccount[]) {
  if (!canUseStorage()) return;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(accounts));
}

function readCurrentAccountId(accounts: RiotMockAccount[]): string {
  if (!canUseStorage()) return accounts[0]?.id ?? "";
  const current = localStorage.getItem(CURRENT_KEY)?.trim() ?? "";
  if (current && accounts.some((account) => account.id === current)) {
    return current;
  }
  return accounts[0]?.id ?? "";
}

function saveCurrentAccountId(accountId: string) {
  if (!canUseStorage()) return;
  localStorage.setItem(CURRENT_KEY, accountId);
}

function createAvatarSvg(account: RiotMockAccount): string {
  const initials = account.displayName
    .split(/\s+/)
    .map((part) => part[0] ?? "")
    .join("")
    .slice(0, 2)
    .toUpperCase();
  const hue = Array.from(account.id).reduce((sum, char) => sum + char.charCodeAt(0), 0) % 360;
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

export function getRiotAccounts(): RiotMockAccount[] {
  return readAccounts();
}

export function getRiotStartupSnapshot(): { accounts: RiotMockAccount[]; currentAccount: string } {
  const accounts = readAccounts();
  const currentAccount = readCurrentAccountId(accounts);
  return { accounts, currentAccount };
}

export function switchRiotAccount(accountId: string) {
  saveCurrentAccountId(accountId);
}

export function addRiotAccount(): RiotMockAccount {
  const existing = readAccounts();
  const nextIndex = existing.length + 1;
  const regions = ["EUW", "NA", "APAC", "KR"];
  const region = regions[(nextIndex - 1) % regions.length];
  const account: RiotMockAccount = {
    id: `riot-${region.toLowerCase()}-${nextIndex.toString().padStart(2, "0")}`,
    username: `Agent${nextIndex}#${region}`,
    displayName: `Mock Agent ${nextIndex}`,
    region,
    tagLine: region,
    lastLoginAt: Date.now(),
  };
  const accounts = [...existing, account];
  saveAccounts(accounts);
  saveCurrentAccountId(account.id);
  return account;
}

export function forgetRiotAccount(accountId: string) {
  const accounts = readAccounts().filter((account) => account.id !== accountId);
  saveAccounts(accounts);
  const current = readCurrentAccountId(accounts);
  saveCurrentAccountId(current);
}

export function getRiotProfile(accountId: string): PlatformProfileInfo | null {
  const account = readAccounts().find((entry) => entry.id === accountId);
  if (!account) return null;
  return {
    avatarUrl: createAvatarSvg(account),
    displayName: account.displayName,
  };
}

export function getCachedRiotProfile(accountId: string) {
  const profile = getRiotProfile(accountId);
  if (!profile?.avatarUrl) return null;
  return {
    url: profile.avatarUrl,
    displayName: profile.displayName ?? undefined,
    expired: false,
  };
}

export function getRiotRegion(accountId: string): string | null {
  return readAccounts().find((account) => account.id === accountId)?.region ?? null;
}
