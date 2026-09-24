import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn(async (_command: string) => false);
vi.mock("@tauri-apps/api/core", () => ({ invoke: (command: string) => invoke(command) }));

import { createStreamerModeController } from "./useStreamerMode.svelte";
import type { AppSettings } from "$lib/features/settings/types";

describe("streamer mode polling", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invoke.mockClear();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  function detections() {
    return invoke.mock.calls.filter(([command]) => command === "detect_streaming_software").length;
  }

  it("polls every few seconds while visible and far less while hidden", async () => {
    let hidden = false;
    const controller = createStreamerModeController({
      getSettings: () => ({ streamerMode: "auto" }) as AppSettings,
      setStreamerMode: () => {},
      isHidden: () => hidden,
      now: () => Date.now(),
    });
    controller.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(detections()).toBe(1);

    await vi.advanceTimersByTimeAsync(8_000);
    expect(detections()).toBe(3);

    hidden = true;
    await vi.advanceTimersByTimeAsync(28_000);
    // One more poll at most in 28 s, against seven while visible.
    expect(detections()).toBeLessThanOrEqual(4);

    hidden = false;
    await vi.advanceTimersByTimeAsync(4_000);
    expect(detections()).toBeGreaterThanOrEqual(4);
    controller.stop();
  });
});
