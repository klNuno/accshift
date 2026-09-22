import "./app.css";
import App from "./App.svelte";
import { invoke } from "@tauri-apps/api/core";
import { mount } from "svelte";
import { fetchBootPayload } from "$lib/app/bootPayload";
import { initializeClientStorage } from "$lib/storage/clientStorage";
import { loadLocaleMessages } from "$lib/i18n";
import { getSettings } from "$lib/features/settings/store";
import { bootMarks, markBoot } from "$lib/app/bootMarks";
import { getBootPayload } from "$lib/app/bootPayload";
import { getInitialActiveTab } from "$lib/app/platformShell.svelte";
import { preloadPlatformModule } from "$lib/platforms/registry";
import { toRuntimeOs } from "$lib/shared/platform";
import { recordFontWarmup } from "$lib/app/fontWarmup";
import { rememberBootLocale } from "$lib/app/bootLocale";

type LogLevel = "info" | "warn" | "error";
type LogPayload = {
  level: LogLevel;
  source: string;
  message: string;
  details: string | null;
};

const originalConsoleError = console.error.bind(console);
let bootFinished = false;
let logChain = Promise.resolve();
let lastLoggingFailureAt = 0;
/**
 * Info records written while the app boots wait here until the window is up.
 * Each one is an IPC round trip whose reply is evaluated on the webview's main
 * thread, in front of the mount; six of them were landing between the boot
 * payload and `finish_boot`. Anything above info goes out immediately and
 * takes the queue with it, so a failure still arrives with its context.
 */
let bootLogBuffer: LogPayload[] | null = [];

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
  const payload: LogPayload = {
    level,
    source,
    message: message.slice(0, 512),
    details: details ? details.slice(0, 16_384) : null,
  };

  if (bootLogBuffer && level === "info") {
    bootLogBuffer.push(payload);
    return;
  }
  flushBootLogs();
  sendLog(payload);
}

function flushBootLogs() {
  const pending = bootLogBuffer;
  if (!pending) return;
  bootLogBuffer = null;
  for (const payload of pending) sendLog(payload);
}

function sendLog(payload: LogPayload) {
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

/**
 * When the webview finished its first frame, which `finish_boot` cannot say:
 * it lands before the compositor has taken a single one. The animation frame
 * callback runs at the start of that frame; the task queued from it runs once
 * style, layout and paint are done. Unlike the Paint Timing API, this also
 * fires in a window that is not on screen yet.
 */
function reportFirstFrame() {
  requestAnimationFrame(() => {
    setTimeout(() => {
      queueLog(
        "info",
        "frontend.paint",
        "First frame",
        String(Math.round(performance.now() * 10) / 10),
      );
    }, 0);
  });
}

async function finishBoot(source: string) {
  if (bootFinished) return;
  bootFinished = true;
  markBoot("finishBoot");
  const completed = invoke("finish_boot", { source, marks: bootMarks() });
  reportFirstFrame();
  if (getBootPayload()?.runtimeOs === "windows") {
    // Once the first frame is out and the page has nothing left to do.
    requestAnimationFrame(() =>
      requestIdleCallback(() => recordFontWarmup(document.body), { timeout: 2000 }),
    );
  }
  try {
    await completed;
    flushBootLogs();
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

markBoot("mainTs");
queueLog("info", "frontend.boot", "main.ts initialized");
// A boot that never completes, and never errors either, would keep its info
// records in memory: the log has to show what the last session did.
window.setTimeout(flushBootLogs, 5000);

let app;

async function bootstrap() {
  try {
    // One round trip for everything boot needs (storage, themes, runtime OS,
    // migration result). On failure the legacy per-command path below covers.
    await fetchBootPayload();
    markBoot("bootPayload");
    queueLog("info", "frontend.boot", "Boot payload loaded");
  } catch (reason) {
    queueLog("error", "frontend.boot", "Failed to load boot payload", serializeLogValue(reason));
  }

  try {
    await initializeClientStorage();
    markBoot("clientStorage");
    queueLog("info", "frontend.storage", "Client storage initialized");
  } catch (reason) {
    queueLog(
      "error",
      "frontend.storage",
      "Failed to initialize client storage",
      serializeLogValue(reason),
    );
  }

  // The first thing the window shows is the active platform's accounts, and
  // their adapter is a chunk of its own that App only asks for once it has
  // mounted. Asked for here, it downloads while the locale loads and the app
  // mounts. Same choice as the shell's, which also knows the OS by then.
  preloadPlatformModule(
    getInitialActiveTab(getSettings(), toRuntimeOs(getBootPayload()?.runtimeOs)),
  );

  try {
    // Non-EN dictionaries live in their own lazy chunk. Await the persisted
    // locale BEFORE the first render so a French user never sees an English
    // flash; for "en" this resolves synchronously.
    const language = getSettings().language;
    rememberBootLocale(language);
    await loadLocaleMessages(language);
    markBoot("locale");
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
    markBoot("mounted");
    queueLog("info", "frontend.boot", "App mounted");
  } catch (reason) {
    queueLog("error", "frontend.mount", "Failed to mount App", serializeLogValue(reason));
    throw reason;
  }
}

void bootstrap();

export default app;
