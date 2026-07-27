/**
 * The fake IPC itself: scenario selection, runtime controls, and the guard that
 * keeps a click from reaching the real backend.
 *
 * Two invariants this file exists to enforce:
 *
 * 1. **Nothing real on screen, nothing real touched.** Every account, avatar and
 *    folder is invented, and every unhandled `platform_*` / `steam_*` / `riot_*`
 *    / `roblox_*` command is refused rather than forwarded. A real switch would
 *    stop Steam, rewrite the autologin in HKCU and relaunch the client.
 * 2. **A `demo` build ignores every runtime override.** That scenario produces
 *    `docs/demo-*.webp`; a leftover `__mock.use("huge")` in localStorage must
 *    not be able to poison a take days later.
 *
 * Boot plumbing the mock does not fake (`log_app_event`, `finish_boot`, window
 * controls) still goes to the real backend — the window would never show up
 * otherwise.
 */
import { type Handler, createHandlers, delay, generateAccounts } from "./fixtures";
import { AVATARS, DEFAULT_SCENARIO, SCENARIOS } from "./scenarios";

/** Injected by vite.config.js from VITE_MOCK / VITE_DEMO. */
declare const __MOCK_SCENARIO__: string;

const BUILD_SCENARIO = SCENARIOS[__MOCK_SCENARIO__] ? __MOCK_SCENARIO__ : DEFAULT_SCENARIO;
/** The recording dataset takes no runtime instruction. See invariant 2 above. */
const FROZEN = BUILD_SCENARIO === "demo";

const OVERRIDES_KEY = "accshift.mock.overrides";

interface MockOverrides {
  scenario?: string;
  runtimeOs?: string;
  latencyMs?: number;
  /** command name -> error message thrown instead of running the handler. */
  failures?: Record<string, string>;
  /** client.* / cache.* stores merged over the scenario's own. */
  stores?: Record<string, unknown>;
  /** Replaces the scenario's accounts with N generated ones. */
  accountCount?: number;
}

function readOverrides(): MockOverrides {
  if (FROZEN) return {};
  try {
    const raw = window.localStorage.getItem(OVERRIDES_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? (parsed as MockOverrides) : {};
  } catch {
    return {};
  }
}

function writeOverrides(next: MockOverrides) {
  try {
    window.localStorage.setItem(OVERRIDES_KEY, JSON.stringify(next));
  } catch {
    console.warn("[mock] could not persist overrides");
  }
}

const overrides = readOverrides();
const scenarioId =
  !FROZEN && overrides.scenario && SCENARIOS[overrides.scenario]
    ? overrides.scenario
    : BUILD_SCENARIO;

const spec = SCENARIOS[scenarioId]();
if (overrides.runtimeOs) spec.runtimeOs = overrides.runtimeOs;
if (typeof overrides.accountCount === "number") {
  spec.steamAccounts = generateAccounts(Math.max(0, overrides.accountCount), AVATARS);
  spec.currentSteamAccount = spec.steamAccounts[0]?.steam_id ?? "";
  // The scenario's folder layout points at account ids that no longer exist.
  // Dropping it beats leaving dangling references in the tree.
  delete spec.stores["client.folders"];
}
if (overrides.stores) spec.stores = { ...spec.stores, ...overrides.stores };

const handlers: Record<string, Handler> = createHandlers(spec);

/** Live knobs: changing these takes effect on the next call, without a reload. */
let latencyMs = overrides.latencyMs ?? 0;
const failures: Record<string, string> = { ...overrides.failures };

// Commands that reach a real platform. Anything here without a handler is
// refused rather than forwarded: a stray click must not touch Steam, the
// keyring or the registry.
const GUARDED_PREFIXES = ["platform_", "steam_", "riot_", "roblox_", "cs2_bridge_", "telemetry_"];

interface TauriInternals {
  invoke: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>;
}

/** Real IPC, for the boot plumbing the mock does not fake. */
function realInvoke<T>(cmd: string, args?: unknown, options?: unknown): Promise<T> {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: TauriInternals })
    .__TAURI_INTERNALS__;
  if (!internals) {
    return Promise.reject(new Error("__TAURI_INTERNALS__ unavailable"));
  }
  return internals.invoke(cmd, args, options) as Promise<T>;
}

/** Drop-in replacement for `invoke` from `@tauri-apps/api/core`. */
export async function mockInvoke<T>(cmd: string, args?: unknown, options?: unknown): Promise<T> {
  // hasOwn, not a plain lookup: a command named like an Object.prototype member
  // would otherwise resolve to it and run instead of hitting the guard below.
  if (Object.hasOwn(failures, cmd)) {
    throw new Error(failures[cmd]);
  }
  if (Object.hasOwn(handlers, cmd)) {
    const handler = handlers[cmd];
    if (latencyMs > 0) await delay(latencyMs);
    return (await handler((args ?? {}) as Record<string, unknown>)) as T;
  }
  if (GUARDED_PREFIXES.some((prefix) => cmd.startsWith(prefix))) {
    console.warn(`[mock] blocked un-mocked command: ${cmd}`);
    throw new Error(`mock mode: ${cmd} is not available`);
  }
  return realInvoke<T>(cmd, args, options);
}

// ------------------------------------------------------------ window.__mock

function refuseWhenFrozen(): boolean {
  if (FROZEN) console.warn("[mock] demo build: runtime overrides are ignored on purpose");
  return FROZEN;
}

function persist(patch: MockOverrides, reload: boolean) {
  if (refuseWhenFrozen()) return;
  writeOverrides({ ...readOverrides(), ...patch });
  if (reload) window.location.reload();
}

const api = {
  /** Which datasets exist, with their one-line description. */
  scenarios: () =>
    Object.fromEntries(Object.entries(SCENARIOS).map(([id, build]) => [id, build().label])),
  /** What the current page is actually running. */
  state: () => ({
    buildScenario: BUILD_SCENARIO,
    scenario: scenarioId,
    frozen: FROZEN,
    runtimeOs: spec.runtimeOs,
    accounts: spec.steamAccounts.length,
    riotProfiles: spec.riotProfiles.length,
    latencyMs,
    failures: { ...failures },
    stores: Object.keys(spec.stores),
  }),
  /** Switch dataset (reloads). */
  use: (id: string) => {
    if (!SCENARIOS[id]) {
      console.warn(`[mock] unknown scenario: ${id}. Known: ${Object.keys(SCENARIOS).join(", ")}`);
      return;
    }
    persist({ scenario: id }, true);
  },
  /** Replace the account list with N generated ones, any scenario (reloads). */
  accounts: (count: number) => persist({ accountCount: count }, true),
  /** Pretend to run on another OS — titlebar, traffic lights, paths (reloads). */
  os: (name: "windows" | "macos" | "linux") => persist({ runtimeOs: name }, true),
  /** Start from a given client store, e.g. seed("client.settings", {themeId:"glass"}) (reloads). */
  seed: (storeId: string, value: unknown) =>
    persist({ stores: { ...readOverrides().stores, [storeId]: value } }, true),
  /** Delay every mocked command, to look at skeletons and spinners (live). */
  latency: (ms: number) => {
    // Checked before mutating: these two apply live, so a warn-after-the-fact
    // would leave a demo build actually changed.
    if (refuseWhenFrozen()) return latencyMs;
    latencyMs = Math.max(0, ms);
    persist({ latencyMs }, false);
    return latencyMs;
  },
  /** Force a command to reject, to reach the error UI (live). Omit message to clear. */
  fail: (cmd: string, message?: string) => {
    if (refuseWhenFrozen()) return { ...failures };
    if (message === undefined) delete failures[cmd];
    else failures[cmd] = message;
    persist({ failures: { ...failures } }, false);
    return { ...failures };
  },
  /** Back to the build scenario, no overrides (reloads). */
  reset: () => {
    if (FROZEN) return;
    try {
      window.localStorage.removeItem(OVERRIDES_KEY);
    } catch {
      /* nothing to clear */
    }
    window.location.reload();
  },
};

declare global {
  interface Window {
    __mock?: typeof api;
  }
}

export function installMockApi() {
  window.__mock = api;
  console.info(
    `[mock] IPC mocked — scenario "${scenarioId}" (${spec.label})${FROZEN ? " [frozen]" : ""}\n` +
      "[mock] __mock.state() · .scenarios() · .use(id) · .accounts(n) · .os(name) · " +
      ".seed(store, value) · .latency(ms) · .fail(cmd, msg) · .reset()",
  );
}
