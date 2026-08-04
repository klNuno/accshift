import { describe, expect, it } from "vitest";
import {
  buildBatch,
  eventTimestamp,
  maskIp,
  readJsonCapped,
  redactUuids,
  type TelemetryEvent,
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
    // The valid entries of a list survive; the rest is discarded.
    expect(item!.properties.enabled_platforms).toEqual(["steam"]);
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
