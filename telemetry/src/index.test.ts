import { describe, expect, it } from "vitest";
import { readJsonCapped } from "./index";

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
