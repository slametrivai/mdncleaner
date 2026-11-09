// Loads the bundled Firebase Analytics lazily, so a missing bundle or no
// internet never stops the app. Events logged while it loads are queued.

let module = null;
let queue = [];

export async function startAnalytics(version) {
  try {
    const loaded = await import("../vendor/analytics.js");
    await loaded.initAnalytics({ version });
    module = loaded;
    for (const [name, params] of queue) module.track(name, params);
  } catch (error) {
    module = null;
    console.warn("analytics unavailable:", error);
  }
  queue = null;
}

/** Anonymous usage event. Never include file names or paths. */
export function track(name, params) {
  if (module) module.track(name, params);
  else queue?.push([name, params]);
}

export const toMb = (bytes) => Math.round(bytes / (1024 * 1024));
