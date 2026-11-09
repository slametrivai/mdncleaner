// "Riwayat": past cleans, what they freed and how long they can be undone.

import { invoke } from "../api.js";
import { $, el, icon, toast } from "../dom.js";
import { DAY_MS, formatBytes, formatDate, formatDayMonth, formatMonth, formatNumber, formatTime } from "../format.js";
import { store, setApp, refreshDrives } from "../store.js";
import { track } from "../analytics.js";

const RECYCLE_DAYS = 7;
const CLOUD_DAYS = 30;

const KIND_TITLES = {
  manual: "Pembersihan manual",
  auto: "Bersih otomatis",
  duplicates: "Hapus duplikat",
  largeFiles: "Hapus file besar",
};
const LOCATION_NAMES = { pc: "PC ini", googleDrive: "Google Drive", icloudDrive: "iCloud Drive" };
const CLOUD_TRASH = { googleDrive: "Trash Google Drive", icloudDrive: "Baru Dihapus iCloud" };
const CLOUD_SHORT = { googleDrive: "Drive", icloudDrive: "iCloud" };

const recycleUntil = (e) => e.atMs + RECYCLE_DAYS * DAY_MS;
const cloudUntil = (e) => e.atMs + CLOUD_DAYS * DAY_MS;
const recycleValid = (e, now) => e.recycleBytes > 0 && !e.purged && !e.restored && now < recycleUntil(e);
const cloudValid = (e, now) => e.cloudBytes > 0 && now < cloudUntil(e);

/** What can still be undone, and how. */
function retention(e, now) {
  if (e.restored) return { text: "Sudah dipulihkan", muted: true };
  const mixed = e.recycleBytes > 0 && e.cloudBytes > 0;
  if (recycleValid(e, now)) {
    return { text: `Recycle Bin sampai ${formatDayMonth(recycleUntil(e))}`, action: "restore" };
  }
  if (cloudValid(e, now)) {
    const text = mixed
      ? `${CLOUD_SHORT[e.cloud]} ${formatBytes(e.cloudBytes, 1)} sampai ${formatDayMonth(cloudUntil(e))}`
      : `${CLOUD_TRASH[e.cloud]} sampai ${formatDayMonth(cloudUntil(e))}`;
    return { text, action: "cloud" };
  }
  return { text: "Sudah dihapus permanen", muted: true };
}

function restorableBytes(e, now) {
  if (e.restored) return 0;
  return (recycleValid(e, now) ? e.recycleBytes : 0) + (cloudValid(e, now) ? e.cloudBytes : 0);
}

function renderStats(history, now) {
  const last30 = history.filter((e) => e.atMs >= now - 30 * DAY_MS);
  const prev30 = history.filter((e) => e.atMs < now - 30 * DAY_MS && e.atMs >= now - 60 * DAY_MS);
  const freed = last30.reduce((s, e) => s + e.bytes, 0);
  const diff = freed - prev30.reduce((s, e) => s + e.bytes, 0);

  $("stat-freed").textContent = formatBytes(freed);
  $("stat-freed-note").textContent =
    last30.length === 0
      ? "Belum ada pembersihan"
      : `${diff >= 0 ? "+" : "−"}${formatBytes(Math.abs(diff), 1)} dari 30 hari sebelumnya`;

  const auto = last30.filter((e) => e.kind === "auto").length;
  $("stat-runs").textContent = formatNumber(last30.length);
  $("stat-runs-note").textContent = `${formatNumber(last30.length - auto)} manual · ${formatNumber(auto)} otomatis`;

  const restorable = history.filter((e) => restorableBytes(e, now) > 0);
  $("stat-restorable").textContent = formatBytes(restorable.reduce((s, e) => s + restorableBytes(e, now), 0));
  $("stat-restorable-note").textContent = `Dari ${formatNumber(restorable.length)} pembersihan`;
}

async function restore(entry) {
  try {
    const { restored, state } = await invoke("restore", { id: entry.id });
    setApp(state);
    track("restore_used", { items: restored });
    toast(`${formatNumber(restored)} item dikembalikan ke tempat asalnya.`);
    refreshDrives();
  } catch (error) {
    toast(String(error));
    setApp(await invoke("get_state").catch(() => null));
  }
}

async function openCloudTrash(entry) {
  try {
    await invoke("open_cloud_trash", { location: entry.cloud });
  } catch (error) {
    toast(`Trash tidak bisa dibuka: ${error}`);
  }
}

function row(e, now) {
  const keep = retention(e, now);
  return el(
    "div",
    { className: "history-row" },
    el(
      "div",
      { className: "history-when" },
      el("span", { className: "history-date", text: formatDate(e.atMs) }),
      el("span", { className: "history-time", text: formatTime(e.atMs) }),
    ),
    el(
      "div",
      { className: "history-what" },
      el("span", { className: "history-title", text: KIND_TITLES[e.kind] ?? "Pembersihan" }),
      el("div", { className: "chips" }, ...e.locations.map((l) => el("span", { className: "chip", text: LOCATION_NAMES[l] ?? l }))),
    ),
    el(
      "div",
      { className: "history-size" },
      el("span", { className: "history-bytes", text: formatBytes(e.bytes, 1) }),
      el("span", { className: "history-files", text: `${formatNumber(e.files)} file` }),
    ),
    el(
      "div",
      { className: "history-keep" },
      el("span", { className: keep.muted ? "history-retention is-muted" : "history-retention", text: keep.text }),
      keep.action
        ? el(
            "button",
            {
              type: "button",
              className: "btn-small btn-small-icon",
              onclick: () => (keep.action === "restore" ? restore(e) : openCloudTrash(e)),
            },
            icon("rotateCcw", 14),
            "Pulihkan",
          )
        : null,
    ),
  );
}

function render() {
  const now = Date.now();
  const history = store.app?.history ?? [];
  renderStats(history, now);

  const recent = history.filter((e) => e.atMs >= now - 30 * DAY_MS);
  const list = $("history-list");
  if (recent.length === 0) {
    list.replaceChildren(el("p", { className: "list-empty", text: "Belum ada pembersihan dalam 30 hari terakhir." }));
    return;
  }
  const children = [];
  let month = null;
  for (const e of recent) {
    const label = formatMonth(e.atMs);
    if (label !== month) {
      month = label;
      children.push(el("div", { className: "month-header", text: label }));
    }
    children.push(row(e, now));
  }
  list.replaceChildren(...children);
}

export const historyView = {
  id: "history",
  title: "Riwayat",
  init() {},
  async show() {
    // Pick up scheduled cleans and expired items.
    setApp(await invoke("get_state").catch(() => null));
  },
  render,
};
