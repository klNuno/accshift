import { afterEach, describe, expect, it, vi } from "vitest";
import clientSource from "../../crates/accshift-core/src/telemetry/client.rs?raw";
import eventsSource from "../../crates/accshift-core/src/telemetry/events.rs?raw";
import platformsSource from "../../crates/accshift-core/src/platforms/mod.rs?raw";
import worker, {
  ANIMATION_MODES,
  buildBatch,
  CLI_COMMANDS,
  cors,
  ERROR_CODES,
  EVENT_NAMES,
  eventTimestamp,
  maskIp,
  OPERATIONS,
  PLATFORM_IDS,
  readJsonCapped,
  redactUuids,
  STREAMER_MODES,
  type Env,
  type TelemetryEvent,
  UI_LANGUAGES,
  usableEvents,
} from "./index";

function streamingRequest(chunks: Uint8Array[], contentLength?: number): Request {
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(chunk);
      controller.close();
    },
  });
  const headers = new Headers({ "Content-Type": "application/json" });
  if (contentLength !== undefined) headers.set("Content-Length", String(contentLength));
  return new Request("https://telemetry.invalid/track", {
    method: "POST",
    headers,
    body: stream,
    duplex: "half",
  } as RequestInit);
}

describe("readJsonCapped", () => {
  it("parses a streamed JSON payload below the byte cap", async () => {
    const encoder = new TextEncoder();
    const result = await readJsonCapped<{ ok: boolean }>(
      streamingRequest([encoder.encode('{"ok":'), encoder.encode("true}")]),
      32,
    );

    expect(result).toEqual({ ok: true });
  });

  it("rejects a declared oversized body before reading it", async () => {
    const result = await readJsonCapped(streamingRequest([], 65), 64);

    expect(result).toBeInstanceOf(Response);
    expect((result as Response).status).toBe(413);
  });

  it("stops a chunked body as soon as its byte cap is crossed", async () => {
    const encoder = new TextEncoder();
    const result = await readJsonCapped(
      streamingRequest([encoder.encode("12345678"), encoder.encode("9")]),
      8,
    );

    expect(result).toBeInstanceOf(Response);
    expect((result as Response).status).toBe(413);
  });
});

describe("buildBatch", () => {
  const IDS = { eventIdentifier: "daily-rotating-hash", pingIdentifier: "stable-ping-hash" };
  const TS = "2026-01-15T10:00:00.000Z";

  const ping: TelemetryEvent = { name: "ping", app_version: "1.2.3", os_version: "Windows 11" };
  const launch: TelemetryEvent = {
    name: "app_launched",
    app_version: "1.2.3",
    os_version: "Windows 11",
    locale: "fr_FR",
    duration_ms: 420,
  };

  it("keeps Mode A off person profiles entirely", () => {
    const batch = buildBatch("A", [ping, launch], IDS, "FR", TS);

    for (const item of batch) {
      expect(item.properties.$process_person_profile).toBe(false);
      expect(item.properties.$set).toBeUndefined();
    }
  });

  it("gives Mode A a stable identifier for ping and a rotating one for usage", () => {
    const [pingItem, launchItem] = buildBatch("A", [ping, launch], IDS, "FR", TS);

    // Counting unique installations needs an identifier that survives the
    // night; linking two usage events across days must stay impossible.
    expect(pingItem!.distinct_id).toBe("stable-ping-hash");
    expect(launchItem!.distinct_id).toBe("daily-rotating-hash");
  });

  it("uses the install_id for every Mode B event and attaches person properties", () => {
    const ids = { eventIdentifier: "install-uuid", pingIdentifier: "install-uuid" };
    const batch = buildBatch("B", [ping, launch], ids, "FR", TS);

    for (const item of batch) {
      expect(item.distinct_id).toBe("install-uuid");
      expect(item.properties.$process_person_profile).toBe(true);
    }
    expect(batch[1]!.properties.$set).toEqual({
      app_version: "1.2.3",
      os_version: "Windows 11",
      country: "FR",
      locale: "fr_FR",
    });
  });

  it("suppresses IP storage and GeoIP on every event of both modes", () => {
    const batch = [
      ...buildBatch("A", [ping, launch], IDS, "FR", TS),
      ...buildBatch("B", [ping, launch], IDS, "FR", TS),
    ];

    for (const item of batch) {
      // Truthy on purpose. PostHog back-fills $ip from the request socket on
      // any falsy value, so null would store an address instead of hiding one.
      expect(item.properties.$ip).toBe("0.0.0.0");
      expect(item.properties.$ip).toBeTruthy();
      expect(item.properties.$geoip_disable).toBe(true);
    }
  });

  it("sends exactly the documented properties and nothing else", () => {
    // The guard that matters: a new field added upstream cannot reach PostHog
    // without this failing first and forcing someone to look at it.
    const [item] = buildBatch("A", [launch], IDS, "FR", TS);

    expect(Object.keys(item!.properties).sort()).toEqual([
      "$geoip_disable",
      "$ip",
      "$process_person_profile",
      "app_version",
      "country",
      "distinct_id",
      "duration_ms",
      "locale",
      "os_version",
      "telemetry_mode",
    ]);
  });

  it("omits optional fields instead of sending them empty", () => {
    const [item] = buildBatch("A", [ping], IDS, "FR", TS);

    expect(item!.properties).not.toHaveProperty("locale");
    expect(item!.properties).not.toHaveProperty("platform");
    expect(item!.properties).not.toHaveProperty("duration_ms");
    expect(item!.properties).not.toHaveProperty("count");
  });

  it("forwards the new event properties the app reports", () => {
    const failed: TelemetryEvent = {
      name: "platform_switch",
      app_version: "1.2.3",
      os_version: "Windows 11",
      os: "windows",
      arch: "x86_64",
      surface: "gui",
      platform: "battle-net",
      duration_ms: 900,
      count: 0,
      success: false,
      error_code: "client_running",
    };
    const [item] = buildBatch("A", [failed], IDS, "FR", TS);

    expect(item!.properties.os).toBe("windows");
    expect(item!.properties.arch).toBe("x86_64");
    expect(item!.properties.surface).toBe("gui");
    // The dashed registry id has to survive verbatim; renaming it on the wire
    // would break the dashboards a second time.
    expect(item!.properties.platform).toBe("battle-net");
    expect(item!.properties.success).toBe(false);
    expect(item!.properties.error_code).toBe("client_running");
  });

  it("drops a property whose shape is not the one the app sends", () => {
    // A modified client is the threat here: the app maps these onto closed
    // vocabularies, and this is the half of that guarantee that does not
    // depend on the client being ours.
    const hostile = {
      name: "operation_failed",
      app_version: "1.2.3",
      os_version: "Windows 11",
      operation: "account_add",
      error_code: "C:\\Users\\alice\\steam missing",
      platform: "steam; DROP TABLE",
      enabled_platforms: ["steam", "../../etc/passwd"],
      duration_ms: -5,
      target_version: "<script>alert(1)</script>",
    } as unknown as TelemetryEvent;
    const [item] = buildBatch("A", [hostile], IDS, "FR", TS);

    expect(item!.properties.operation).toBe("account_add");
    expect(item!.properties).not.toHaveProperty("error_code");
    expect(item!.properties).not.toHaveProperty("platform");
    expect(item!.properties).not.toHaveProperty("duration_ms");
    expect(item!.properties).not.toHaveProperty("target_version");
    // Settings fields are Mode B only, so Mode A drops the list even when an
    // entry is a real platform id. A shape failure is still omitted, not
    // rewritten, which is what the assertions above lock in.
    expect(item!.properties).not.toHaveProperty("enabled_platforms");
  });

  it("drops Mode B snapshots and settings fields from a Mode A batch", () => {
    const usage: TelemetryEvent = {
      name: "app_launched",
      app_version: "1.2.3",
      os_version: "Windows 11",
      ui_language: "fr",
      enabled_platforms: ["steam"],
      personas_enabled: true,
      pin_enabled: true,
      cli_enabled: false,
      deep_links_enabled: true,
      streamer_mode: "off",
      animations: "on",
    };
    const batch = buildBatch(
      "A",
      [
        usage,
        {
          name: "accounts_snapshot",
          app_version: "1.2.3",
          os_version: "Windows 11",
          platform: "steam",
          count: 2,
        },
        {
          name: "settings_snapshot",
          app_version: "1.2.3",
          os_version: "Windows 11",
          pin_enabled: true,
        },
      ],
      IDS,
      "FR",
      TS,
    );

    expect(batch.map((item) => item.event)).toEqual(["app_launched"]);
    expect(batch[0]!.properties).not.toHaveProperty("ui_language");
    expect(batch[0]!.properties).not.toHaveProperty("enabled_platforms");
    expect(batch[0]!.properties).not.toHaveProperty("personas_enabled");
    expect(batch[0]!.properties).not.toHaveProperty("pin_enabled");
    expect(batch[0]!.properties).not.toHaveProperty("cli_enabled");
    expect(batch[0]!.properties).not.toHaveProperty("deep_links_enabled");
    expect(batch[0]!.properties).not.toHaveProperty("streamer_mode");
    expect(batch[0]!.properties).not.toHaveProperty("animations");
  });

  it("maps a shape-valid code outside the app vocabulary onto other", () => {
    const hostile = {
      name: "operation_failed",
      app_version: "1.2.3",
      os_version: "Windows 11",
      operation: "account_add",
      error_code: "alice_home_folder",
      platform: "not_a_platform",
      command: "rm_rf_home",
      ui_language: "klingon",
      streamer_mode: "always",
      animations: "fancy",
    } as TelemetryEvent;
    const [item] = buildBatch("B", [hostile], IDS, "FR", TS);

    expect(item!.properties.operation).toBe("account_add");
    expect(item!.properties.error_code).toBe("other");
    expect(item!.properties.platform).toBe("other");
    expect(item!.properties.command).toBe("other");
    expect(item!.properties.ui_language).toBe("other");
    expect(item!.properties.streamer_mode).toBe("other");
    expect(item!.properties.animations).toBe("other");
  });

  it("maps an unknown platform id inside a Mode B settings list onto other", () => {
    const [item] = buildBatch(
      "B",
      [
        {
          name: "settings_snapshot",
          app_version: "1.2.3",
          os_version: "Windows 11",
          enabled_platforms: ["steam", "alice_home", "../../etc/passwd"],
        },
      ],
      IDS,
      "FR",
      TS,
    );

    expect(item!.properties.enabled_platforms).toEqual(["steam", "other"]);
  });

  it("keeps a settings snapshot as booleans and codes", () => {
    const snapshot: TelemetryEvent = {
      name: "settings_snapshot",
      app_version: "1.2.3",
      os_version: "Windows 11",
      ui_language: "pt_br",
      enabled_platforms: ["steam", "riot"],
      personas_enabled: true,
      pin_enabled: false,
      cli_enabled: true,
      deep_links_enabled: true,
      streamer_mode: "auto",
      animations: "system",
    };
    const [item] = buildBatch("B", [snapshot], IDS, "BR", TS);

    expect(item!.properties.ui_language).toBe("pt_br");
    expect(item!.properties.enabled_platforms).toEqual(["steam", "riot"]);
    expect(item!.properties.pin_enabled).toBe(false);
    // False must survive: `if (ev.pin_enabled)` would have dropped it.
    expect(item!.properties).toHaveProperty("pin_enabled");
  });

  it("stamps each event with the instant it happened", () => {
    const first = { ...launch, client_ts: "2026-01-15T09:58:01Z" };
    const second = { ...launch, client_ts: "2026-01-15T09:59:47Z" };
    const batch = buildBatch("A", [first, second], IDS, "FR", TS);

    // Both events arrived in the same batch; flattening them onto TS would
    // lose the ordering and the two minutes between them.
    expect(batch[0]!.timestamp).toBe("2026-01-15T09:58:01Z");
    expect(batch[1]!.timestamp).toBe("2026-01-15T09:59:47Z");
  });

  it("falls back to arrival time when the client clock is unusable", () => {
    const batch = buildBatch(
      "A",
      [
        { ...launch, client_ts: "2031-01-15T10:00:00Z" },
        { ...launch, client_ts: "not-a-timestamp" },
        { ...launch },
      ],
      IDS,
      "FR",
      TS,
    );

    for (const item of batch) expect(item.timestamp).toBe(TS);
  });
});

describe("eventTimestamp", () => {
  const SERVER = "2026-01-15T10:00:00.000Z";

  it("trusts a plausible client timestamp", () => {
    expect(eventTimestamp("2026-01-15T09:55:00Z", SERVER)).toBe("2026-01-15T09:55:00Z");
  });

  it("rejects a timestamp beyond a day of skew in either direction", () => {
    expect(eventTimestamp("2026-01-13T10:00:00Z", SERVER)).toBe(SERVER);
    expect(eventTimestamp("2026-01-17T10:00:00Z", SERVER)).toBe(SERVER);
  });

  it("rejects anything that is not the exact wire format", () => {
    expect(eventTimestamp("2026-01-15T10:00:00.123Z", SERVER)).toBe(SERVER);
    expect(eventTimestamp("2026-01-15 10:00:00", SERVER)).toBe(SERVER);
    expect(eventTimestamp(42, SERVER)).toBe(SERVER);
    expect(eventTimestamp(undefined, SERVER)).toBe(SERVER);
  });
});

describe("redactUuids", () => {
  it("strips an install_id out of an upstream error message", () => {
    const err =
      "status 400: SyntaxError in query SELECT timestamp FROM events " +
      "WHERE distinct_id = '9f8b1c2d-3e4f-4a5b-8c9d-0e1f2a3b4c5d'";
    const redacted = redactUuids(err);
    expect(redacted).not.toContain("9f8b1c2d");
    expect(redacted).toContain("<uuid>");
    // The rest of the message has to survive, otherwise the log is useless.
    expect(redacted).toContain("status 400");
  });

  it("leaves a message without a uuid untouched", () => {
    expect(redactUuids("status 503: upstream busy")).toBe("status 503: upstream busy");
  });
});

describe("maskIp", () => {
  it("keeps only the /24 of an IPv4 address", () => {
    expect(maskIp("203.0.113.42")).toBe("203.0.113.x");
  });

  it("keeps only the /48 of a full IPv6 address", () => {
    expect(maskIp("2001:db8:85a3:1:2:3:4:5")).toBe("2001:db8:85a3::/48");
  });

  it("pads a compressed IPv6 address instead of emitting a malformed prefix", () => {
    expect(maskIp("2001:db8::1")).toBe("2001:db8:0::/48");
  });

  it("never returns an address for an empty or unparseable input", () => {
    expect(maskIp("")).toBe("unknown");
    expect(maskIp("not-an-ip")).toBe("unknown");
  });
});

describe("cors", () => {
  const env = { ALLOWED_ORIGINS: "https://accshift.app,https://dash.accshift.app" } as Env;

  function corsHeaders(origin?: string): Headers {
    const headers = new Headers();
    if (origin !== undefined) headers.set("Origin", origin);
    const request = new Request("https://telemetry.invalid/track", { method: "POST", headers });
    return cors(new Response(null, { status: 204 }), request, env).headers;
  }

  it("omits the allow-origin header entirely when the request carries no Origin", () => {
    const headers = corsHeaders();

    expect(headers.has("Access-Control-Allow-Origin")).toBe(false);
    // The rest of the preflight answer still has to be there.
    expect(headers.get("Access-Control-Allow-Methods")).toBe("GET, POST, OPTIONS");
  });

  it("refuses the literal null origin a sandboxed iframe sends", () => {
    expect(corsHeaders("null").has("Access-Control-Allow-Origin")).toBe(false);
  });

  it("echoes an allow-listed origin and varies on it", () => {
    const headers = corsHeaders("https://accshift.app");

    expect(headers.get("Access-Control-Allow-Origin")).toBe("https://accshift.app");
    expect(headers.get("Vary")).toBe("Origin");
  });

  it("says nothing about an origin that is not on the list", () => {
    expect(corsHeaders("https://evil.invalid").has("Access-Control-Allow-Origin")).toBe(false);
  });
});

function rustVocabulary(source: string, name: string): string[] {
  const start = source.indexOf(`pub const ${name}`);
  if (start < 0) throw new Error(`missing ${name}`);
  const slice = source.slice(start, source.indexOf("];", start));
  const values = [...slice.matchAll(/"([a-z0-9_]+)"/g)].map((match) => match[1]!);
  if (slice.includes("UNKNOWN_CODE") && !values.includes("other")) values.push("other");
  return values;
}

function withOther(values: string[]): string[] {
  return values.includes("other") ? values : [...values, "other"];
}

describe("closed vocabularies", () => {
  it("matches the codes the Rust client is allowed to send", () => {
    expect(new Set(rustVocabulary(eventsSource, "ERROR_CODES"))).toEqual(ERROR_CODES);
    expect(new Set(rustVocabulary(eventsSource, "OPERATIONS"))).toEqual(OPERATIONS);
    expect(new Set(rustVocabulary(eventsSource, "CLI_COMMANDS"))).toEqual(CLI_COMMANDS);
    expect(new Set(rustVocabulary(eventsSource, "UI_LANGUAGES"))).toEqual(UI_LANGUAGES);
  });

  it("matches the platform ids and the settings enums", () => {
    const idsBlock = platformsSource.slice(
      platformsSource.indexOf("pub mod ids"),
      platformsSource.indexOf("pub const ALL"),
    );
    const ids = [...idsBlock.matchAll(/pub const [A-Z0-9_]+: &str = "([a-z0-9-]+)";/g)].map(
      (match) => match[1]!,
    );
    expect(new Set(ids)).toEqual(PLATFORM_IDS);

    const list = (arg: string) => {
      const matched = clientSource.match(new RegExp(`code_from\\(${arg}, &\\[([^\\]]+)\\]\\)`));
      if (!matched?.[1]) throw new Error(`missing code_from(${arg})`);
      return withOther([...matched[1].matchAll(/"([a-z0-9_]+)"/g)].map((item) => item[1]!));
    };
    expect(new Set(list("streamer_mode"))).toEqual(STREAMER_MODES);
    expect(new Set(list("animations"))).toEqual(ANIMATION_MODES);
  });
});

describe("usableEvents", () => {
  it("keeps only the event names the app can emit", () => {
    const kept = usableEvents([
      { name: "ping" },
      { name: "platform_switch" },
      { name: "$identify" },
      { name: "made_up_event" },
      { name: "" },
      { name: 42 },
      null,
      "ping",
    ]);

    expect(kept.map((e) => e.name)).toEqual(["ping", "platform_switch"]);
  });

  it("matches every name Event::name can return in the Rust client", () => {
    const body = eventsSource.slice(
      eventsSource.indexOf("pub fn name(&self)"),
      eventsSource.indexOf("pub fn is_mode_b_only"),
    );
    const rustNames = [...body.matchAll(/=> "([a-z0-9_]+)"/g)].map((m) => m[1]!);

    expect(rustNames.length).toBeGreaterThan(10);
    expect(new Set(rustNames)).toEqual(new Set(EVENT_NAMES));
  });
});

describe("buildBatch field validation", () => {
  const IDS = { eventIdentifier: "e", pingIdentifier: "p" };
  const TS = "2026-01-15T10:00:00.000Z";

  it("drops free-text versions and locales that do not fit their shape", () => {
    const [item] = buildBatch(
      "B",
      [
        {
          name: "app_launched",
          app_version: "1.2.3<script>",
          os_version: "x".repeat(65),
          locale: "fr_FR; DROP",
        },
      ],
      IDS,
      "FR",
      TS,
    );

    expect(item!.properties.app_version).toBe("");
    expect(item!.properties.os_version).toBe("");
    expect(item!.properties.locale).toBeUndefined();
    expect(item!.properties.$set).toEqual({ app_version: "", os_version: "", country: "FR" });
  });

  it("keeps well-formed versions and locales", () => {
    const [item] = buildBatch(
      "A",
      [
        {
          name: "ping",
          app_version: "1.0.4",
          os_version: "Windows 11 Pro 10.0.26200",
          locale: "pt-BR",
        },
      ],
      IDS,
      "FR",
      TS,
    );

    expect(item!.properties.app_version).toBe("1.0.4");
    expect(item!.properties.os_version).toBe("Windows 11 Pro 10.0.26200");
    expect(item!.properties.locale).toBe("pt-BR");
  });
});

const INSTALL = "9f8b1c2d-3e4f-4a5b-8c9d-0e1f2a3b4c5d";

function workerEnv(): Env {
  const allow = { limit: async () => ({ success: true }) };
  return {
    RL_TRACK: allow,
    RL_RGPD: allow,
    RL_GLOBAL: allow,
    RL_NOTIFY: allow,
    HASH_SECRET: "test-secret-value",
    POSTHOG_PROJECT_API_KEY: "phc_test",
    POSTHOG_PERSONAL_API_KEY: "phx_test",
    POSTHOG_PROJECT_ID: "1",
    POSTHOG_INGEST_HOST: "https://ingest.posthog.test",
    POSTHOG_API_HOST: "https://eu.posthog.test",
    RESEND_API_KEY: "",
    ALERT_EMAIL: "",
    ALERT_FROM: "",
    ENVIRONMENT: "test",
    BATCH_MAX_EVENTS: "50",
    ALLOWED_ORIGINS: "",
    UA_PREFIX: "Accshift/",
  };
}

const workerCtx = {
  waitUntil() {},
  passThroughOnException() {},
} as unknown as ExecutionContext;

function postJson(path: string, body: unknown): Request {
  return new Request(`https://telemetry.invalid${path}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "User-Agent": "Accshift/1.0.4 (telemetry)",
      "CF-Connecting-IP": "203.0.113.10",
    },
    body: JSON.stringify(body),
  });
}

describe("privacy proxy endpoints", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("omits a consent app_version that is not a version", async () => {
    let forwarded: Record<string, unknown> | undefined;
    vi.stubGlobal("fetch", (async (_input: RequestInfo | URL, init?: RequestInit) => {
      const parsed = JSON.parse(String(init?.body)) as {
        batch: Array<{ properties: Record<string, unknown> }>;
      };
      forwarded = parsed.batch[0]?.properties;
      return new Response("ok", { status: 200 });
    }) as typeof fetch);

    const res = await worker.fetch(
      postJson("/consent", { choice: "refused", app_version: "alice@home.example" }),
      workerEnv(),
      workerCtx,
    );

    expect(res.status).toBe(200);
    expect(forwarded).toBeDefined();
    expect(forwarded).not.toHaveProperty("app_version");
    expect(forwarded?.choice).toBe("refused");
  });

  it("keeps a consent app_version that matches the version shape", async () => {
    let forwarded: Record<string, unknown> | undefined;
    vi.stubGlobal("fetch", (async (_input: RequestInfo | URL, init?: RequestInit) => {
      const parsed = JSON.parse(String(init?.body)) as {
        batch: Array<{ properties: Record<string, unknown> }>;
      };
      forwarded = parsed.batch[0]?.properties;
      return new Response("ok", { status: 200 });
    }) as typeof fetch);

    const res = await worker.fetch(
      postJson("/consent", { choice: "basic", app_version: "1.0.4" }),
      workerEnv(),
      workerCtx,
    );

    expect(res.status).toBe(200);
    expect(forwarded?.app_version).toBe("1.0.4");
  });

  it("redacts a uuid echoed in a capture or consent failure", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.stubGlobal("fetch", (async () => {
      return new Response(`distinct_id=${INSTALL}`, { status: 400 });
    }) as typeof fetch);

    const track = await worker.fetch(
      postJson("/track", {
        mode: "A",
        events: [{ name: "ping", app_version: "1.0.4", os_version: "Windows 11" }],
      }),
      workerEnv(),
      workerCtx,
    );
    const consent = await worker.fetch(
      postJson("/consent", { choice: "refused", app_version: "1.0.4" }),
      workerEnv(),
      workerCtx,
    );

    expect(track.status).toBe(502);
    expect(consent.status).toBe(502);
    const logged = spy.mock.calls.map((args) => args.map(String).join(" ")).join("\n");
    expect(logged).not.toContain(INSTALL);
    expect(logged).toContain("<uuid>");
  });

  it("fails an export when the person lookup fails", async () => {
    vi.stubGlobal("fetch", (async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/query/")) {
        return new Response(JSON.stringify({ results: [] }), { status: 200 });
      }
      return new Response("lookup failed", { status: 500 });
    }) as typeof fetch);

    const res = await worker.fetch(
      postJson("/export", { install_id: INSTALL }),
      workerEnv(),
      workerCtx,
    );
    const body = (await res.json()) as { error?: string; person_properties?: unknown };

    expect(res.status).toBe(502);
    expect(body.error).toBe("upstream_unavailable");
    expect(body).not.toHaveProperty("person_properties");
  });
});
