import "./app.css";
import App from "./App.svelte";
import { invoke } from "@tauri-apps/api/core";
import { mount } from "svelte";
import { fetchBootPayload } from "$lib/app/bootPayload";
import { initializeClientStorage } from "$lib/storage/clientStorage";
import { loadLocaleMessages } from "$lib/i18n";
import { getSettings } from "$lib/features/settings/store";

type LogLevel = "info" | "warn" | "error";

const originalConsoleError = console.error.bind(console);
let bootFinished = false;
let logChain = Promise.resolve();
let lastLoggingFailureAt = 0;

function serializeLogValue(value: unknown, seen = new WeakSet<object>()): string {
  if (value instanceof Error) {
    return JSON.stringify({
      name: value.name,
      message: value.message,
      stack: value.stack,
      cause: value.cause ? serializeLogValue(value.cause, seen) : undefined,
    });
  }

  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean" || value == null)
    return String(value);
  if (typeof value === "bigint") return value.toString();

  if (typeof value === "object") {
    if (seen.has(value)) return "[Circular]";
    seen.add(value);
    try {
      return JSON.stringify(value);
    } catch {
      return Object.prototype.toString.call(value);
    }
  }

  return String(value);
}

function queueLog(level: LogLevel, source: string, message: string, details?: string | null) {
  const payload = {
    level,
    source,
    message: message.slice(0, 512),
    details: details ? details.slice(0, 16_384) : null,
  };

  logChain = logChain
    .catch(() => {})
    .then(async () => {
      try {
        await invoke("log_app_event", payload);
      } catch (reason) {
        const now = Date.now();
        if (now - lastLoggingFailureAt > 5000) {
          lastLoggingFailureAt = now;
          originalConsoleError("App logging unavailable:", reason);
        }
      }
    });
}

async function finishBoot(source: string) {
  if (bootFinished) return;
  bootFinished = true;
  try {
    await invoke("finish_boot", { source });
  } catch (reason) {
    bootFinished = false;
    queueLog("error", "frontend.finish_boot", "Failed to finish boot", serializeLogValue(reason));
    originalConsoleError("Failed to finish boot:", reason);
  }
}

window.addEventListener(
  "accshift:boot-ready",
  () => {
    queueLog("info", "frontend.boot", "Received boot-ready signal");
    void finishBoot("frontend.boot-ready");
  },
  { once: true },
);

window.addEventListener(
  "load",
  () => {
    queueLog("info", "frontend.boot", "Window load event fired");
    window.setTimeout(() => {
      if (bootFinished) return;
      queueLog("warn", "frontend.boot", "1500ms fallback elapsed before boot-ready");
      void finishBoot("frontend.load-fallback-1500ms");
    }, 1500);
  },
  { once: true },
);

window.addEventListener("error", (event) => {
  const message = event.message || "Unhandled window error";
  const details = serializeLogValue({
    filename: event.filename,
    lineno: event.lineno,
    colno: event.colno,
    error: event.error,
  });
  queueLog("error", "frontend.window.error", message, details);
});

window.addEventListener("unhandledrejection", (event) => {
  const details = serializeLogValue(event.reason);
  queueLog("error", "frontend.unhandledrejection", "Unhandled promise rejection", details);
});

console.error = (...args: unknown[]) => {
  originalConsoleError(...args);
  queueLog(
    "error",
    "frontend.console.error",
    serializeLogValue(args[0] ?? "console.error"),
    args.length > 1 ? serializeLogValue(args.slice(1)) : null,
  );
};

queueLog("info", "frontend.boot", "main.ts initialized");

let app;

async function bootstrap() {
  try {
    // One round trip for everything boot needs (storage, themes, runtime OS,
    // migration result). On failure the legacy per-command path below covers.
    await fetchBootPayload();
    queueLog("info", "frontend.boot", "Boot payload loaded");
  } catch (reason) {
    queueLog("error", "frontend.boot", "Failed to load boot payload", serializeLogValue(reason));
  }

  try {
    await initializeClientStorage();
    queueLog("info", "frontend.storage", "Client storage initialized");
  } catch (reason) {
    queueLog(
      "error",
      "frontend.storage",
      "Failed to initialize client storage",
      serializeLogValue(reason),
    );
  }

  try {
    // Non-EN dictionaries live in their own lazy chunk. Await the persisted
    // locale BEFORE the first render so a French user never sees an English
    // flash; for "en" this resolves synchronously.
    await loadLocaleMessages(getSettings().language);
  } catch (reason) {
    // Non-fatal: translate() falls back to English and retries the load.
    queueLog(
      "error",
      "frontend.i18n",
      "Failed to preload locale messages",
      serializeLogValue(reason),
    );
  }

  try {
    app = mount(App, {
      target: document.getElementById("app")!,
    });
    queueLog("info", "frontend.boot", "App mounted");
  } catch (reason) {
    queueLog("error", "frontend.mount", "Failed to mount App", serializeLogValue(reason));
    throw reason;
  }
}

void bootstrap();

export default app;
