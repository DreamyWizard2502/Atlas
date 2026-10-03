// Send frontend errors to the persistent crash log (logs/atlas.log in Atlas's
// data folder), so they're on disk even when no terminal or devtools is open.
import { api } from "./api";

const seen = new Set<string>();
let sent = 0;
/** Per-session cap, so a render loop can't flood the log. */
const MAX_PER_SESSION = 50;

export function reportError(source: string, error: unknown, componentStack?: string | null) {
  const err = error instanceof Error ? error : new Error(typeof error === "string" ? error : JSON.stringify(error));
  const key = `${source}|${err.message}|${(componentStack ?? "").slice(0, 200)}`;
  if (seen.has(key) || sent >= MAX_PER_SESSION) return;
  seen.add(key);
  sent++;
  api
    .logClientError({ source, message: `${err.name}: ${err.message}`, stack: err.stack ?? null, componentStack: componentStack ?? null })
    .catch(() => {
      // Logging must never throw back into the UI.
    });
}

/** Uncaught errors and unhandled promise rejections outside React rendering. */
export function installGlobalErrorLogging() {
  window.addEventListener("error", (e) => reportError("window", e.error ?? e.message));
  window.addEventListener("unhandledrejection", (e) => reportError("promise", e.reason));
}
