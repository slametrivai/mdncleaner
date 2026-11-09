// State shared by every page, with a tiny change feed.

import { invoke } from "./api.js";

export const store = {
  app: null, // AppState from the backend
  drives: [], // fixed drives
  drivesCheckedAt: 0,
  locations: { googleDrive: null, icloudDrive: null }, // CloudStatus
  items: [], // Pembersih scan result
  scanning: false,
  cleaning: false,
};

const listeners = new Set();
export const subscribe = (fn) => listeners.add(fn);
export function notify() {
  for (const fn of listeners) fn();
}

export function setApp(app) {
  if (app) {
    store.app = app;
    notify();
  }
}

export async function refreshDrives() {
  try {
    store.drives = await invoke("drives");
    store.drivesCheckedAt = Date.now();
  } catch (error) {
    console.error("drives", error);
  }
  notify();
}

export async function refreshLocation(location) {
  try {
    store.locations[location] = await invoke("cloud_status", { location });
  } catch (error) {
    console.error("cloud_status", error);
  }
  notify();
}

/** "Tanya dulu sebelum menghapus": a native confirmation, when switched on. */
export async function confirmDelete(title, message, ok) {
  if (store.app?.settings?.confirmBeforeDelete === false) return true;
  return invoke("confirm", { title, message, ok }).catch(() => false);
}

export const systemDrive =() => store.drives.find((d) => d.system) ?? store.drives[0];

/** Checked junk total from the last Pembersih scan. */
export function checkedJunk() {
  const selection = store.app?.selection ?? {};
  return store.items
    .filter((item) => selection[item.id] ?? item.defaultChecked)
    .reduce((sum, item) => sum + item.bytes, 0);
}
