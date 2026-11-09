// "Pengaturan": preferences, saved as soon as they change.

import { invoke, getVersion } from "../api.js";
import { $, el, icon, toast } from "../dom.js";
import { DAY_MS, formatWhen } from "../format.js";
import { store, setApp, refreshLocation } from "../store.js";
import { track } from "../analytics.js";
import { updateRow } from "../update.js";

const SECTIONS = [
  { id: "umum", label: "Umum" },
  { id: "pembersihan", label: "Pembersihan" },
  { id: "otomatis", label: "Bersih otomatis" },
  { id: "cloud", label: "Lokasi cloud" },
  { id: "notifikasi", label: "Notifikasi" },
  { id: "pembaruan", label: "Pembaruan" },
];

const LANGUAGES = [["id", "Bahasa Indonesia"]];
const THEMES = [
  ["light", "Terang"],
  ["dark", "Gelap"],
  ["system", "Ikuti Windows"],
];
const RECENT = [
  [1, "1 jam terakhir"],
  [24, "24 jam terakhir"],
  [72, "3 hari terakhir"],
  [168, "7 hari terakhir"],
];
const LARGE = [
  [100, "Lebih dari 100 MB"],
  [250, "Lebih dari 250 MB"],
  [500, "Lebih dari 500 MB"],
  [1024, "Lebih dari 1 GB"],
  [5120, "Lebih dari 5 GB"],
];
const SCHEDULES = [
  ["daily", "Setiap hari, 20.00"],
  ["weekly", "Setiap Minggu, 20.00"],
  ["monthly", "Setiap tanggal 1, 20.00"],
];

let version = "1.0.0";
let active = "umum";
let built = false;

const settings = () => store.app?.settings ?? {};

async function save(patch) {
  try {
    setApp(await invoke("set_settings", { settings: { ...settings(), ...patch } }));
  } catch (error) {
    toast(`Pengaturan tidak bisa disimpan: ${error}`);
    render();
  }
}

// ── Controls ───────────────────────────────────────────────

function toggle(label, checked, onChange) {
  return el(
    "button",
    {
      type: "button",
      className: "switch",
      role: "switch",
      "aria-checked": String(Boolean(checked)),
      "aria-label": label,
      onclick: (event) => {
        event.currentTarget.disabled = true;
        onChange(!checked);
      },
    },
    el("span", { className: "switch-knob" }),
  );
}

function select(label, options, value, onChange) {
  const node = el(
    "select",
    { "aria-label": label, onchange: (event) => onChange(event.target.value) },
    ...options.map(([v, text]) => el("option", { value: String(v), selected: String(v) === String(value), text })),
  );
  return el("div", { className: "select" }, node, icon("chevronDown", 16));
}

function row(label, desc, control) {
  return el(
    "div",
    { className: "setting-row" },
    el(
      "div",
      { className: "setting-text" },
      el("span", { className: "setting-label", text: label }),
      el("span", { className: "setting-desc", text: desc }),
    ),
    control,
  );
}

function button(iconName, label, onClick) {
  return el("button", { type: "button", className: "btn-outline btn-outline-sm", onclick: onClick }, icon(iconName, 15), label);
}

function card(section, ...rows) {
  return el(
    "section",
    { className: "settings-card", id: `settings-${section.id}`, "aria-labelledby": `settings-${section.id}-title` },
    el("h2", { className: "settings-card-title", id: `settings-${section.id}-title`, text: section.label }),
    ...rows,
  );
}

// ── Sections ───────────────────────────────────────────────

function nextRun(schedule, now = new Date()) {
  const next = new Date(now);
  next.setHours(20, 0, 0, 0);
  if (schedule === "daily") {
    if (next <= now) next.setDate(next.getDate() + 1);
  } else if (schedule === "weekly") {
    next.setDate(next.getDate() + ((7 - next.getDay()) % 7));
    if (next <= now) next.setDate(next.getDate() + 7);
  } else {
    next.setDate(1);
    if (next <= now) next.setMonth(next.getMonth() + 1);
  }
  const day = new Intl.DateTimeFormat("id-ID", { weekday: "long" }).format(next);
  const date = new Intl.DateTimeFormat("id-ID", { day: "numeric", month: "short", year: "numeric" }).format(next);
  return `${day}, ${date}, 20.00`;
}

async function setAutoClean(enabled) {
  try {
    setApp(await invoke("set_auto_clean", { enabled }));
    track("auto_clean_set", { enabled });
  } catch (error) {
    toast(`Jadwal tidak bisa diubah: ${error}`);
    render();
  }
}

function autoLocations() {
  const s = settings();
  const locations = s.autoLocations ?? {};
  const chips = [
    ["pc", "PC ini", true],
    ["googleDrive", "Google Drive", store.locations.googleDrive?.connected],
    ["icloudDrive", "iCloud Drive", store.locations.icloudDrive?.connected],
  ];
  return el(
    "div",
    { className: "chip-group" },
    ...chips.map(([key, label, available]) => {
      const on = Boolean(available && locations[key]);
      return el(
        "button",
        {
          type: "button",
          className: on ? "chip-toggle is-on" : "chip-toggle",
          "aria-pressed": String(on),
          disabled: !available,
          title: available ? null : `${label} belum terhubung`,
          onclick: () => save({ autoLocations: { ...locations, [key]: !on } }),
        },
        on ? icon("check", 13) : null,
        label,
      );
    }),
  );
}

function cloudRow(location, name, app) {
  const status = store.locations[location];
  if (status?.connected) {
    const how = status.manual ? "dipilih manual" : "terdeteksi otomatis";
    return row(
      `Folder ${name}`,
      `${status.root} · ${how}`,
      button("folderOpen", "Ubah folder", async () => {
        try {
          setApp(await invoke("pick_cloud_folder", { location }));
          await refreshLocation(location);
        } catch (error) {
          toast(String(error));
        }
      }),
    );
  }
  return row(
    `Folder ${name}`,
    status ? `Belum ditemukan. Pasang ${app} dulu.` : "Mengecek…",
    button("refresh", "Cek lagi", async () => {
      await refreshLocation(location);
      toast(store.locations[location]?.connected ? `${name} terhubung.` : `${name} belum ditemukan.`);
    }),
  );
}

async function reset() {
  const ok = await invoke("confirm", {
    title: "Kembalikan ke pengaturan awal?",
    message: "Semua pengaturan, folder yang dilewati, dan pilihan scan kembali ke awal. Bersih otomatis dimatikan.\n\nRiwayat dan file kamu tidak ikut terhapus.",
    ok: "Reset",
  }).catch(() => false);
  if (!ok) return;
  try {
    setApp(await invoke("reset_settings"));
    toast("Pengaturan dikembalikan ke awal.");
  } catch (error) {
    toast(String(error));
  }
}

function cards() {
  const s = settings();
  const auto = Boolean(store.app?.autoClean);
  const [umum, pembersihan, otomatis, cloud, notifikasi, pembaruan] = SECTIONS;
  return [
    card(
      umum,
      row(
        "Jalankan saat Windows menyala",
        "Berjalan diam-diam di tray supaya bersih otomatis bisa jalan.",
        toggle("Jalankan saat Windows menyala", s.startWithWindows, (v) => save({ startWithWindows: v })),
      ),
      row("Bahasa", "Bahasa tampilan aplikasi.", select("Bahasa", LANGUAGES, s.language, (v) => save({ language: v }))),
      row(
        "Tema",
        "Terang, gelap, atau ikut pengaturan Windows.",
        el(
          "div",
          { className: "theme-switch", role: "radiogroup", "aria-label": "Tema" },
          ...THEMES.map(([value, label]) =>
            el("button", {
              type: "button",
              role: "radio",
              className: s.theme === value ? "theme-option is-active" : "theme-option",
              "aria-checked": String(s.theme === value),
              text: label,
              onclick: () => save({ theme: value }),
            }),
          ),
        ),
      ),
    ),
    card(
      pembersihan,
      row(
        "Tanya dulu sebelum menghapus",
        "Tampilkan ringkasan file yang akan dihapus dan minta konfirmasi.",
        toggle("Tanya dulu sebelum menghapus", s.confirmBeforeDelete, (v) => save({ confirmBeforeDelete: v })),
      ),
      row(
        "Kirim ke Recycle Bin dulu",
        "File bisa dipulihkan 7 hari. Kalau dimatikan, file langsung dihapus permanen.",
        toggle("Kirim ke Recycle Bin dulu", s.useRecycleBin, (v) => save({ useRecycleBin: v })),
      ),
      row(
        "Lewati file yang baru dipakai",
        "File yang diubah dalam rentang ini tidak disentuh.",
        select("Lewati file yang baru dipakai", RECENT, s.skipRecentHours, (v) => save({ skipRecentHours: Number(v) })),
      ),
      row(
        "Batas file besar",
        "File di atas ukuran ini muncul di Analisis disk.",
        select("Batas file besar", LARGE, s.largeFileMb, (v) => save({ largeFileMb: Number(v) })),
      ),
    ),
    card(
      otomatis,
      row(
        "Nyalakan bersih otomatis",
        "Analisis dan bersihkan sesuai jadwal, tanpa perlu buka aplikasi.",
        toggle("Nyalakan bersih otomatis", auto, setAutoClean),
      ),
      row(
        "Jadwal",
        auto ? `Jalan berikutnya: ${nextRun(s.schedule)}.` : "Nyalakan bersih otomatis untuk memakai jadwal ini.",
        select("Jadwal", SCHEDULES, s.schedule, (v) => save({ schedule: v })),
      ),
      row("Lokasi yang dibersihkan", "Cloud hanya dicek duplikatnya, tidak dihapus otomatis.", autoLocations()),
      row(
        "Hanya saat laptop dicas",
        "Tidak jalan saat pakai baterai supaya hemat daya.",
        toggle("Hanya saat laptop dicas", s.onlyOnAc, (v) => save({ onlyOnAc: v })),
      ),
    ),
    card(
      cloud,
      cloudRow("googleDrive", "Google Drive", "Google Drive for desktop"),
      cloudRow("icloudDrive", "iCloud Drive", "iCloud for Windows"),
      row(
        "Cek lokasi cloud setiap app dibuka",
        "Mendeteksi folder Google Drive dan iCloud yang baru dipasang.",
        toggle("Cek lokasi cloud setiap app dibuka", s.checkCloudOnStart, (v) => save({ checkCloudOnStart: v })),
      ),
    ),
    card(
      notifikasi,
      row(
        "Ingatkan kalau sampah menumpuk",
        "Notifikasi Windows muncul saat sampah lebih dari 5 GB.",
        toggle("Ingatkan kalau sampah menumpuk", s.remindJunk, (v) => save({ remindJunk: v })),
      ),
      row(
        "Kabari setelah bersih otomatis",
        "Ringkasan singkat berapa yang dibebaskan.",
        toggle("Kabari setelah bersih otomatis", s.notifyAfterAuto, (v) => save({ notifyAfterAuto: v })),
      ),
    ),
    card(
      pembaruan,
      row(
        "Cek pembaruan otomatis",
        "Kami kabari kalau ada versi baru. Pemasangan tetap atas izin kamu.",
        toggle("Cek pembaruan otomatis", s.autoUpdateCheck, (v) => save({ autoUpdateCheck: v })),
      ),
      updateVersionRow(),
    ),
    el(
      "div",
      { className: "reset-row" },
      el(
        "div",
        { className: "setting-text" },
        el("span", { className: "setting-label", text: "Kembalikan ke pengaturan awal" }),
        el("span", { className: "setting-desc", text: "Riwayat dan file kamu tidak ikut terhapus." }),
      ),
      button("rotateCcw", "Reset", reset),
    ),
  ];
}

function updateVersionRow() {
  const state = updateRow(formatWhen);
  const control = button("refresh", state.label, state.action ?? (() => {}));
  control.disabled = Boolean(state.busy);
  return row(`Versi ${version}`, state.desc, control);
}

// ── Layout and scroll-spy ──────────────────────────────────

function setActive(id) {
  active = id;
  for (const item of document.querySelectorAll(".settings-index-item")) {
    const on = item.dataset.section === id;
    item.classList.toggle("is-active", on);
    if (on) item.setAttribute("aria-current", "true");
    else item.removeAttribute("aria-current");
  }
}

function onScroll() {
  const section = document.querySelector('[data-view="settings"]');
  if (!section || section.hidden) return;
  const top = $("content").getBoundingClientRect().top + 80;
  let current = SECTIONS[0].id;
  for (const s of SECTIONS) {
    const node = $(`settings-${s.id}`);
    if (node && node.getBoundingClientRect().top <= top) current = s.id;
  }
  if (current !== active) setActive(current);
}

function build() {
  const section = document.querySelector('[data-view="settings"]');
  section.replaceChildren(
    el(
      "div",
      { className: "page-heading" },
      el("h1", { className: "page-title", text: "Pengaturan" }),
      el("p", { className: "page-sub", text: "Atur cara MDN Cleaner membersihkan. Perubahan tersimpan otomatis." }),
    ),
    el(
      "div",
      { className: "settings-layout" },
      el(
        "nav",
        { className: "settings-index", "aria-label": "Bagian pengaturan" },
        ...SECTIONS.map((s) =>
          el("button", {
            type: "button",
            className: "settings-index-item",
            "data-section": s.id,
            text: s.label,
            onclick: () => {
              $(`settings-${s.id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
              setActive(s.id);
            },
          }),
        ),
      ),
      el("div", { className: "settings-cards", id: "settings-cards" }),
    ),
  );
  built = true;
}

function render() {
  const section = document.querySelector('[data-view="settings"]');
  if (!section || section.hidden || !store.app) return;
  if (!built) build();
  const scroll = $("content").scrollTop;
  $("settings-cards").replaceChildren(...cards());
  $("content").scrollTop = scroll;
  setActive(active);
}

export const settingsView = {
  id: "settings",
  title: "Pengaturan",
  init() {
    getVersion()
      .then((v) => {
        version = v;
      })
      .catch(() => {});
    $("content").addEventListener("scroll", onScroll, { passive: true });
  },
  show() {
    active = "umum";
    render();
  },
  render,
};

// Re-render once a day so "Jalan berikutnya" stays right if the app stays open.
setInterval(() => settingsView.render(), DAY_MS / 24);
