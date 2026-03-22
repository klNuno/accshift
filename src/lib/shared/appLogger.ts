import { invoke } from "@tauri-apps/api/core";

export type AppLogLevel = "info" | "warn" | "error";

const REDACTED_KEYS = new Set([
  "username",
  "requestedusername",
  "autologinuser",
  "currentaccountfromloginusers",
  "email",
  "targetemail",
  "currentaccount",
  "steamid",
  "accountid",
  "profileid",
  "targetprofileid",
  "currentprofileid",
  "uuid",
  "userid",
]);

function maskValue(val: unknown): unknown {
  if (typeof val === "string") {
    return val.length > 2 ? val.slice(0, 2) + "***" : "***";
  }
  if (Array.isArray(val)) return `[${val.length} items]`;
  return "***";
}

function redactObject(obj: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(obj)) {
    if (REDACTED_KEYS.has(key.toLowerCase())) {
      out[key] = maskValue(val);
    } else if (val && typeof val === "object" && !Array.isArray(val) && !(val instanceof Error)) {
      out[key] = redactObject(val as Record<string, unknown>);
    } else {
      out[key] = val;
    }
  }
  return out;
}

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
    if (value && typeof value === "object") {
      return JSON.stringify(redactObject(value as Record<string, unknown>));
    }
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
