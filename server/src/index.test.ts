import { describe, expect, it } from "vitest";
import { buildBatch, readJsonCapped, type TelemetryEvent } from "./index";

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
});
