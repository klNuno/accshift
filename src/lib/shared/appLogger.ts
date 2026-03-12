import { invoke } from "@tauri-apps/api/core";

export type AppLogLevel = "info" | "warn" | "error";

export function serializeLogValue(value: unknown): string {
  if (value instanceof Error) {
    return JSON.stringify({
      name: value.name,
      message: value.message,
      stack: value.stack,
      cause: value.cause ?? undefined,
    });
  }

  if (typeof value === "string") return value;

  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

export async function logAppEvent(
  level: AppLogLevel,
  source: string,
  message: string,
  details?: unknown,
) {
  try {
    await invoke("log_app_event", {
      level,
      source,
      message,
      details: details == null ? null : serializeLogValue(details),
    });
  } catch {
    // Ignore logging failures here to avoid masking the actual action result.
  }
}
