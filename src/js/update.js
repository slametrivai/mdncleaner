// Pembaruan: one state machine behind the banner, the footer badge, the
// update dialog and the Pengaturan card.
//
//   idle → checking → uptodate | available | error
//   available → downloading → ready | error      (ready → installs and restarts)

import { invoke } from "./api.js";
import { $, el, icon, toast } from "./dom.js";
import { formatBytes, formatDate } from "./format.js";
import { store, notify } from "./store.js";
import { track } from "./analytics.js";

export const update = {
  status: "idle",
  info: null, // UpdateInfo from the backend
  progress: { downloaded: 0, total: null },
  startedAt: 0,
  error: null,
  bannerHidden: false,
};

let currentVersion = "1.0.0";

function changed() {
  renderBanner();
  renderBadge();
  renderModal();
  notify();
}

// ── Actions ────────────────────────────────────────────────

export async function checkForUpdates({ manual = false } = {}) {
  if (update.status === "checking" || update.status === "downloading") {
    if (manual && update.status === "downloading") openModal();
    return;
  }
  if (manual && (update.status === "available" || update.status === "ready")) {
    openModal();
    return;
  }
  update.status = "checking";
  changed();
  try {
    const info = await invoke("check_updates");
    if (store.app) store.app.settings.lastUpdateCheckMs = Date.now();
    if (info) {
      update.info = info;
      update.status = info.ready ? "ready" : "available";
      track("update_found", { version: info.version });
      if (manual) openModal();
    } else {
      update.info = null;
      update.status = "uptodate";
      if (manual) toast(`MDN Cleaner sudah versi terbaru (${currentVersion}).`);
    }
  } catch (error) {
    update.status = "error";
    update.error = String(error);
    if (manual) openModal();
  }
  changed();
}

async function startDownload() {
  update.status = "downloading";
  update.progress = { downloaded: 0, total: null };
  update.startedAt = Date.now();
  changed();
  try {
    await invoke("download_update");
    track("update_download", { version: update.info?.version });
  } catch (error) {
    update.status = "error";
    update.error = String(error);
    changed();
  }
}

async function cancelDownload() {
  await invoke("cancel_update").catch(() => {});
  update.status = "available";
  changed();
}

async function installNow() {
  if (store.cleaning) return;
  try {
    track("update_install", { version: update.info?.version });
    await invoke("install_update");
    // The installer closes the app; nothing after this normally runs.
  } catch (error) {
    update.status = "error";
    update.error = String(error);
    changed();
  }
}

async function installLater() {
  await invoke("install_update_on_exit", { enabled: true }).catch(() => {});
  closeModal();
  toast(`Versi ${update.info.version} dipasang saat MDN Cleaner ditutup.`);
}

// ── Helpers ────────────────────────────────────────────────

/** Bullet points from a GitHub release body, without markdown. */
function noteItems(markdown) {
  if (!markdown) return [];
  const clean = (line) =>
    line
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
      .replace(/[*_`]+/g, "")
      .trim();
  const lines = markdown.split(/\r?\n/).map((l) => l.trim());
  const bullets = lines.filter((l) => /^[-*•]\s+/.test(l)).map((l) => l.replace(/^[-*•]\s+/, ""));
  const items = bullets.length ? bullets : lines.filter((l) => l && !l.startsWith("#"));
  return items.map(clean).filter(Boolean).slice(0, 5);
}

const percent = () => {
  const { downloaded, total } = update.progress;
  return total ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
};

function eta() {
  const { downloaded, total } = update.progress;
  const seconds = (Date.now() - update.startedAt) / 1000;
  if (!total || downloaded === 0 || seconds < 1) return "Menghitung sisa waktu…";
  const left = ((total - downloaded) / (downloaded / seconds)) | 0;
  if (left < 60) return `Sisa sekitar ${Math.max(1, left)} detik`;
  return `Sisa sekitar ${Math.ceil(left / 60)} menit`;
}

const progressText = () => {
  const { downloaded, total } = update.progress;
  return total ? `${formatBytes(downloaded, 1)} dari ${formatBytes(total, 1)}` : formatBytes(downloaded, 1);
};

// ── Banner ─────────────────────────────────────────────────

function renderBanner() {
  const slot = $("update-banner");
  const visible = !update.bannerHidden && ["available", "downloading", "ready"].includes(update.status);
  slot.hidden = !visible;
  if (!visible) return;

  const v = update.info.version;
  const firstNote = noteItems(update.info.notes)[0];
  const content = {
    available: [`Versi ${v} sudah tersedia`, firstNote ?? "Pembaruan siap diunduh dari GitHub.", "Lihat pembaruan"],
    downloading: [`Mengunduh versi ${v}`, `${percent() ?? 0}% · ${progressText()}`, "Lihat progres"],
    ready: [`Versi ${v} siap dipasang`, "Dipasang sekarang, atau otomatis saat aplikasi ditutup.", "Pasang sekarang"],
  }[update.status];

  slot.replaceChildren(
    el(
      "div",
      { className: "update-banner", role: "status" },
      el("span", { className: "update-banner-icon", "aria-hidden": "true" }, icon(update.status === "ready" ? "check" : "download", 18)),
      el(
        "div",
        { className: "update-banner-text" },
        el("span", { className: "update-banner-title", text: content[0] }),
        el("span", { className: "update-banner-desc", text: content[1] }),
      ),
      el("button", {
        type: "button",
        className: "update-banner-later",
        text: "Nanti",
        onclick: () => {
          update.bannerHidden = true;
          renderBanner();
        },
      }),
      el("button", { type: "button", className: "btn-dark btn-dark-sm", text: content[2], onclick: openModal }),
    ),
  );
}

// ── Footer badge ───────────────────────────────────────────

function renderBadge() {
  const badge = $("footer-badge");
  const v = update.info?.version;
  const states = {
    available: ["download", `Versi ${v} tersedia`],
    downloading: ["download", `Mengunduh ${percent() ?? 0}%`],
    ready: ["check", "Siap dipasang"],
  };
  const pending = states[update.status];
  if (pending) {
    badge.hidden = false;
    badge.className = "badge badge-update";
    badge.disabled = false;
    badge.replaceChildren(icon(pending[0], 12), pending[1]);
  } else if (update.status === "uptodate") {
    badge.hidden = false;
    badge.className = "badge";
    badge.disabled = true;
    badge.replaceChildren(icon("check", 12), "Terbaru");
  } else {
    badge.hidden = true;
  }
}

// ── Dialog ─────────────────────────────────────────────────

const dialog = () => $("update-modal");

export function openModal() {
  renderModal();
  if (!dialog().open) dialog().showModal();
}

function closeModal() {
  if (dialog().open) dialog().close();
}

function header(iconName, tone, title, ...rest) {
  return el(
    "div",
    { className: "modal-header" },
    el("button", { type: "button", className: "modal-close", "aria-label": "Tutup", onclick: closeModal }, icon("x", 17)),
    el("span", { className: `update-icon ${tone}`, "aria-hidden": "true" }, icon(iconName, 30)),
    el("h2", { className: "modal-title", id: "update-title", text: title }),
    ...rest,
  );
}

function footer(...buttons) {
  return el("div", { className: "modal-footer modal-footer-end" }, el("div", { className: "modal-actions" }, ...buttons));
}

const outline = (text, onclick) => el("button", { type: "button", className: "btn-outline btn-outline-md", text, onclick });

function availableView() {
  const info = update.info;
  const notes = noteItems(info.notes);
  return [
    header(
      "download",
      "is-koral",
      `Versi ${info.version} tersedia`,
      el(
        "span",
        { className: "version-chip" },
        info.currentVersion,
        el("span", { className: "version-arrow", "aria-label": "ke" }, icon("arrowRight", 13)),
        info.version,
      ),
      info.dateMs ? el("p", { className: "modal-desc", text: `Dirilis ${formatDate(info.dateMs)}` }) : null,
    ),
    el(
      "div",
      { className: "modal-body update-body" },
      el("span", { className: "notes-label", text: "Yang baru" }),
      notes.length
        ? el(
            "ul",
            { className: "notes" },
            ...notes.map((n) => el("li", {}, el("span", { className: "note-check", "aria-hidden": "true" }, icon("check", 12)), n)),
          )
        : el("p", { className: "notes-empty", text: "Catatan rilis ada di halaman GitHub." }),
      el(
        "button",
        {
          type: "button",
          className: "notes-link",
          onclick: () => invoke("open_external", { url: info.releaseUrl }).catch((e) => toast(String(e))),
        },
        "Catatan lengkap di GitHub",
        icon("externalLink", 14),
      ),
      el(
        "p",
        { className: "safety-note" },
        icon("shieldCheck", 15),
        "Pengaturan dan riwayat tetap aman setelah pembaruan",
      ),
    ),
    footer(
      outline("Nanti", closeModal),
      el("button", { type: "button", className: "btn btn-primary btn-modal", text: "Download & pasang", onclick: startDownload }),
    ),
  ];
}

function downloadingView() {
  const p = percent();
  const fill = el("div", { className: "progress-fill" });
  fill.style.width = `${p ?? 4}%`;
  return [
    header(
      "download",
      "is-dark",
      `Mengunduh versi ${update.info.version}`,
      el("p", { className: "modal-desc", text: "Kamu tetap bisa memakai MDN Cleaner selama download berjalan." }),
    ),
    el(
      "div",
      { className: "modal-body update-body" },
      el(
        "div",
        { className: "progress-head" },
        el("span", { className: "progress-bytes", text: progressText() }),
        el("span", { className: "progress-percent", text: p == null ? "…" : `${p}%` }),
      ),
      el(
        "div",
        {
          className: "progress-track",
          role: "progressbar",
          "aria-label": "Progres download",
          "aria-valuemin": "0",
          "aria-valuemax": "100",
          "aria-valuenow": p == null ? null : String(p),
        },
        fill,
      ),
      el("span", { className: "progress-eta", text: `${eta()} · dari github.com/slametrivai` }),
      el(
        "p",
        { className: "info-note" },
        icon("info", 16),
        "Tutup jendela ini kalau mau. Download lanjut di latar dan kami kabari begitu siap dipasang.",
      ),
    ),
    footer(
      outline("Batalkan", cancelDownload),
      el("button", { type: "button", className: "btn-dark", text: "Lanjut di latar", onclick: closeModal }),
    ),
  ];
}

function readyView() {
  const busy = Boolean(store.cleaning);
  return [
    header(
      "check",
      "is-koral",
      `Versi ${update.info.version} siap dipasang`,
      el("p", { className: "modal-desc", text: "MDN Cleaner akan ditutup sebentar lalu terbuka lagi. Prosesnya sekitar 20 detik." }),
    ),
    el(
      "div",
      { className: "modal-body" },
      el(
        "div",
        { className: "info-row info-row-wrap" },
        el("span", { className: "info-tile", "aria-hidden": "true" }, icon("refresh", 17)),
        el("span", { className: "info-text", text: "Kalau ada pembersihan yang sedang jalan, tombol pasang menunggu sampai selesai." }),
      ),
      el(
        "div",
        { className: "info-row info-row-wrap" },
        el("span", { className: "info-tile", "aria-hidden": "true" }, icon("clock", 17)),
        el("span", { className: "info-text", text: "Pilih nanti, dan pembaruan dipasang otomatis saat kamu menutup aplikasi." }),
      ),
    ),
    footer(
      outline("Nanti", installLater),
      el("button", {
        type: "button",
        className: "btn btn-primary btn-modal",
        disabled: busy,
        text: busy ? "Menunggu pembersihan…" : "Pasang & mulai ulang",
        onclick: installNow,
      }),
    ),
  ];
}

function errorView() {
  return [
    header("alert", "is-warm", "Pembaruan belum berhasil", el("p", { className: "modal-desc", text: update.error })),
    footer(
      outline("Tutup", closeModal),
      el("button", {
        type: "button",
        className: "btn-dark",
        text: "Coba lagi",
        onclick: () => {
          update.status = update.info ? "available" : "idle";
          if (update.info) startDownload();
          else checkForUpdates({ manual: true });
        },
      }),
    ),
  ];
}

function renderModal() {
  const node = dialog();
  const views = { available: availableView, downloading: downloadingView, ready: readyView, error: errorView };
  const view = views[update.status];
  if (!view) {
    if (node.open) node.close();
    return;
  }
  // Keep the open dialog's focus target stable while progress ticks.
  const focusedText = node.contains(document.activeElement) ? document.activeElement.textContent : null;
  node.replaceChildren(...view().filter(Boolean));
  if (focusedText) {
    [...node.querySelectorAll("button")].find((b) => b.textContent === focusedText)?.focus();
  }
}

// ── Start-up ───────────────────────────────────────────────

export function initUpdates(version) {
  if (version) currentVersion = version;
  const { listen } = window.__TAURI__.event;
  listen("update-progress", (event) => {
    update.progress = event.payload;
    if (update.status === "downloading") changed();
  });
  listen("update-ready", (event) => {
    update.info = event.payload;
    update.status = "ready";
    update.bannerHidden = false;
    changed();
    if (!dialog().open) toast(`Versi ${update.info.version} siap dipasang.`);
  });
  listen("update-failed", (event) => {
    update.status = "error";
    update.error = String(event.payload);
    changed();
  });
  dialog().addEventListener("click", (event) => {
    if (event.target === dialog()) closeModal();
  });

  renderBadge();
  if (store.app?.settings?.autoUpdateCheck !== false) {
    setTimeout(() => checkForUpdates(), 4000);
    // While the app stays open (also from the tray), look again every 6 hours.
    setInterval(() => {
      if (store.app?.settings?.autoUpdateCheck !== false) checkForUpdates();
    }, 6 * 60 * 60 * 1000);
  }
}

/** Text and action for the Pengaturan › Pembaruan card. */
export function updateRow(formatWhen) {
  const v = update.info?.version;
  const last = store.app?.settings?.lastUpdateCheckMs;
  switch (update.status) {
    case "checking":
      return { desc: "Mengecek GitHub…", label: "Mengecek…", busy: true };
    case "available":
      return { desc: `Versi ${v} tersedia.`, label: "Lihat pembaruan", action: openModal };
    case "downloading":
      return { desc: `Mengunduh versi ${v}… ${percent() ?? 0}%`, label: "Lihat progres", action: openModal };
    case "ready":
      return { desc: `Versi ${v} siap dipasang.`, label: "Pasang sekarang", action: openModal };
    case "error":
      return { desc: update.error, label: "Coba lagi", action: () => checkForUpdates({ manual: true }) };
    case "uptodate":
      return { desc: `Sudah versi terbaru. Dicek ${formatWhen(last ?? Date.now())}.`, label: "Cek sekarang", action: () => checkForUpdates({ manual: true }) };
    default:
      return {
        desc: last ? `Terakhir dicek ${formatWhen(last)}.` : "Belum pernah dicek.",
        label: "Cek sekarang",
        action: () => checkForUpdates({ manual: true }),
      };
  }
}
