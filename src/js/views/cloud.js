// "Google Drive" and "iCloud Drive": duplicates and big files in the synced
// folder, or the steps to connect it.

import { invoke } from "../api.js";
import { el, icon, checkbox, toast } from "../dom.js";
import { formatBytes, formatNumber, formatWhen } from "../format.js";
import { store, setApp, refreshLocation, confirmDelete } from "../store.js";
import { track, toMb } from "../analytics.js";

const CONFIG = {
  googleDrive: {
    name: "Google Drive",
    icon: "googleDrive",
    app: "Google Drive for Desktop",
    trash: "Trash",
    notice:
      "File yang belum diunduh dilewati saat cari duplikat. File yang dihapus masuk Trash Google Drive dan bisa dikembalikan selama 30 hari.",
    setupTitle: "Hubungkan Google Drive dalam 3 langkah",
    setupIntro:
      "MDN Cleaner membaca folder Google Drive yang ada di PC kamu. Kami tidak pernah meminta login atau kata sandi akun Google.",
    steps: [
      ["Download Google Drive for desktop", "Gratis, langsung dari halaman resmi Google."],
      ["Login dengan akun Google", "Lalu pastikan Google Drive muncul di File Explorer."],
      ["Klik Cek lagi", "Lokasinya langsung terdeteksi, tanpa restart app."],
    ],
    download: "Download Google Drive for desktop",
    downloadUrl: "https://www.google.com/drive/download/",
    afterTitle: "Setelah terhubung",
    after:
      "File yang belum diunduh dilewati saat cari duplikat. File yang dihapus masuk Trash Google Drive dan bisa dikembalikan selama 30 hari.",
    skipTitle: "Nggak pakai Google Drive?",
    skip: "Lewati saja. PC ini dan iCloud Drive tetap bisa dibersihkan seperti biasa.",
    missing: "Folder Google Drive tidak ditemukan. Sudah dicek di semua drive dan folder pengguna.",
  },
  icloudDrive: {
    name: "iCloud Drive",
    icon: "cloud",
    app: "iCloud for Windows",
    trash: "Baru Dihapus",
    notice:
      "File yang belum diunduh dilewati saat cari duplikat. File yang dihapus masuk Baru Dihapus di iCloud dan bisa dikembalikan selama 30 hari.",
    setupTitle: "Hubungkan iCloud Drive dalam 3 langkah",
    setupIntro:
      "MDN Cleaner membaca folder iCloud yang ada di PC kamu. Kami tidak pernah meminta login atau kata sandi Apple ID.",
    steps: [
      ["Download iCloud for Windows", "Gratis dari Microsoft Store, lewat halaman resmi Apple."],
      ["Login dengan Apple ID", "Lalu nyalakan iCloud Drive di app iCloud."],
      ["Klik Cek lagi", "Lokasinya langsung terdeteksi, tanpa restart app."],
    ],
    download: "Download iCloud for Windows",
    downloadUrl: "https://apps.microsoft.com/detail/9pktq5699m62",
    afterTitle: "Setelah terhubung",
    after:
      "File yang belum diunduh dilewati saat cari duplikat. File yang dihapus masuk Baru Dihapus di iCloud dan bisa dikembalikan selama 30 hari.",
    skipTitle: "Nggak pakai iPhone atau Mac?",
    skip: "Lewati saja. PC ini dan Google Drive tetap bisa dibersihkan seperti biasa.",
  },
};

const PREVIEW_ROWS = 4;

function createState() {
  return { scan: null, scanning: false, deleting: false, tab: "duplicates", expanded: false, picked: null, checking: false };
}
const states = { googleDrive: createState(), icloudDrive: createState() };

function defaultPicks(scan) {
  return { duplicates: new Set(scan.duplicates.map((d) => d.id)), large: new Set() };
}

async function runScan(location) {
  const s = states[location];
  if (s.scanning) return;
  s.scanning = true;
  render(location);
  try {
    s.scan = await invoke("cloud_scan", { location });
    s.picked = defaultPicks(s.scan);
    s.expanded = false;
  } catch (error) {
    toast(`Scan gagal: ${error}`);
  }
  s.scanning = false;
  render(location);
}

async function deletePicked(location) {
  const s = states[location];
  const dupIds = [...s.picked.duplicates];
  const files = [...s.picked.large];
  if (dupIds.length + files.length === 0) return;
  const c = CONFIG[location];
  const bytes =
    s.scan.duplicates.filter((d) => s.picked.duplicates.has(d.id)).reduce((sum, d) => sum + d.reclaim, 0) +
    s.scan.large.filter((f) => s.picked.large.has(f.path)).reduce((sum, f) => sum + f.bytes, 0);
  const parts = [];
  if (dupIds.length) parts.push(`${formatNumber(dupIds.length)} grup duplikat (salinan terbaru tetap disimpan)`);
  if (files.length) parts.push(`${formatNumber(files.length)} file besar`);
  const ok = await confirmDelete(
    `Pindahkan ${formatBytes(bytes, 1)} ke ${c.trash}?`,
    `${parts.join(" dan ")} akan dihapus dari ${c.name}. File bisa dikembalikan dari ${c.trash} selama 30 hari.`,
    "Pindahkan",
  );
  if (!ok) return;
  s.deleting = true;
  render(location);
  try {
    const { report, state } = await invoke("cloud_delete", { location, dupIds, files });
    setApp(state);
    track("cloud_cleanup", { location, freed_mb: toMb(report.bytes) });
    const skipped = report.skipped ? ` ${formatNumber(report.skipped)} file dilewati.` : "";
    toast(`${formatBytes(report.bytes)} dipindahkan ke ${CONFIG[location].trash}.${skipped}`);
  } catch (error) {
    toast(`Gagal menghapus: ${error}`);
  }
  s.deleting = false;
  await runScan(location);
}

async function recheck(location) {
  const s = states[location];
  s.checking = true;
  render(location);
  await refreshLocation(location);
  s.checking = false;
  const status = store.locations[location];
  toast(status?.connected ? `${CONFIG[location].name} terhubung.` : `${CONFIG[location].name} belum ditemukan.`);
  render(location);
  if (status?.connected) runScan(location);
}

async function openExternal(url) {
  try {
    await invoke("open_external", { url });
  } catch (error) {
    toast(String(error));
  }
}

// ── Pieces ─────────────────────────────────────────────────

function header(location, status) {
  const c = CONFIG[location];
  const s = states[location];
  const connected = Boolean(status?.connected);
  const pill = connected
    ? el("span", { className: "pill" }, icon("check", 13), "Terhubung")
    : el("span", { className: "pill pill-warn", text: "Belum terhubung" });

  let sub;
  if (connected) {
    const when = s.scan ? `dicek ${formatWhen(s.scan.atMs)}` : "belum discan";
    sub = `${status.root} · lewat ${c.app} · ${when}`;
  } else if (status?.checked?.length) {
    sub = `Folder ${location === "icloudDrive" ? "iCloudDrive" : c.name} tidak ditemukan. Sudah dicek di ${status.checked.join(", ")} dan lokasi sinkronisasi lain.`;
  } else {
    sub = c.missing ?? `Folder ${c.name} tidak ditemukan.`;
  }

  return el(
    "div",
    { className: "page-header page-header-center" },
    el(
      "div",
      { className: "page-identity" },
      el("div", { className: connected ? "icon-tile icon-tile-dark" : "icon-tile" }, icon(c.icon, connected ? 22 : 24)),
      el(
        "div",
        { className: "page-heading-tight" },
        el("div", { className: "title-row" }, el("h1", { className: "page-title", text: c.name }), pill),
        el("p", { className: "page-sub", text: sub }),
      ),
    ),
    connected
      ? el(
          "button",
          { type: "button", className: "btn-outline", disabled: s.scanning || s.deleting, onclick: () => runScan(location) },
          icon("refresh", 15),
          s.scanning ? "Memindai…" : "Scan ulang",
        )
      : null,
  );
}

function stat(label, value, note) {
  return el(
    "div",
    { className: "stat stat-compact card" },
    el("span", { className: "stat-label", text: label }),
    el("span", { className: "stat-value", text: value }),
    el("span", { className: "stat-note", text: note }),
  );
}

function stats(scan) {
  const dash = "–";
  return el(
    "div",
    { className: "grid-3" },
    stat("Isi folder ini", scan ? formatBytes(scan.totalBytes, 1) : dash, scan ? `${formatNumber(scan.totalFiles)} file` : " "),
    stat(
      "Sudah diunduh ke PC",
      scan ? formatBytes(scan.localBytes, 1) : dash,
      scan ? `${formatNumber(scan.localFiles)} file, ikut dicek duplikat` : " ",
    ),
    stat("Hanya online", scan ? `${formatNumber(scan.onlineFiles)} file` : dash, "Dilewati saat cari duplikat"),
  );
}

function folderLine(folders) {
  const unique = [...new Set(folders)];
  if (unique.length <= 3) return unique.join(" · ");
  return `${unique.slice(0, 2).join(" · ")} · ${unique.length - 2} lokasi lain`;
}

function toggle(location, set, id, checked) {
  const s = states[location];
  if (checked) s.picked[set].add(id);
  else s.picked[set].delete(id);
  render(location);
}

function dupRow(location, d) {
  const s = states[location];
  return el(
    "label",
    { className: "result-row" },
    ...checkbox({
      checked: s.picked.duplicates.has(d.id),
      disabled: s.deleting,
      onChange: (v) => toggle(location, "duplicates", d.id, v),
    }),
    icon("copy", 16),
    el(
      "span",
      { className: "result-text" },
      el(
        "span",
        { className: "result-name-line" },
        el("span", { className: "result-name", text: d.name }),
        el("span", { className: "result-count", text: `${d.copies} salinan` }),
      ),
      el("span", { className: "result-paths", text: folderLine(d.folders) }),
    ),
    el("span", { className: "result-size", text: formatBytes(d.reclaim, 1) }),
  );
}

function largeRow(location, f) {
  const s = states[location];
  return el(
    "label",
    { className: "result-row" },
    ...checkbox({
      checked: s.picked.large.has(f.path),
      disabled: s.deleting,
      onChange: (v) => toggle(location, "large", f.path, v),
    }),
    icon("file", 16),
    el(
      "span",
      { className: "result-text" },
      el("span", { className: "result-name-line" }, el("span", { className: "result-name", text: f.name })),
      el("span", { className: "result-paths", text: f.folder }),
    ),
    el("span", { className: "result-size", text: formatBytes(f.bytes, 1) }),
  );
}

function results(location) {
  const s = states[location];
  const c = CONFIG[location];
  const scan = s.scan;

  if (!scan) {
    return el(
      "div",
      { className: "card results" },
      el("p", { className: "list-empty", text: s.scanning ? "Memindai folder… File besar dan duplikat sedang dicari." : "Belum discan." }),
    );
  }

  const isDup = s.tab === "duplicates";
  const tabs = el(
    "div",
    { className: "segmented segmented-light", role: "tablist" },
    ...[
      ["duplicates", `Duplikat · ${formatBytes(scan.dupBytes, 1)}`],
      ["large", `File besar · ${formatBytes(scan.largeBytes, 1)}`],
    ].map(([id, label]) =>
      el("button", {
        type: "button",
        role: "tab",
        className: s.tab === id ? "segment is-active" : "segment",
        "aria-selected": String(s.tab === id),
        text: label,
        onclick: () => {
          s.tab = id;
          s.expanded = false;
          render(location);
        },
      }),
    ),
  );
  const note = isDup
    ? `${formatNumber(scan.dupGroups)} grup duplikat. Salinan terbaru selalu disimpan.`
    : "Centang file yang sudah tidak kamu perlukan.";

  const all = isDup ? scan.duplicates : scan.large;
  const shown = s.expanded ? all : all.slice(0, PREVIEW_ROWS);
  const rows = shown.map((item) => (isDup ? dupRow(location, item) : largeRow(location, item)));
  if (all.length === 0) {
    rows.push(el("p", { className: "list-empty", text: isDup ? "Tidak ada file duplikat." : "Tidak ada file di atas 500 MB." }));
  }

  const pickedBytes =
    scan.duplicates.filter((d) => s.picked.duplicates.has(d.id)).reduce((sum, d) => sum + d.reclaim, 0) +
    scan.large.filter((f) => s.picked.large.has(f.path)).reduce((sum, f) => sum + f.bytes, 0);
  const hidden = all.length - shown.length;

  return el(
    "div",
    { className: "card results" },
    el("div", { className: "results-toolbar" }, tabs, el("span", { className: "card-note", text: note })),
    el("div", { className: "results-rows" }, ...rows),
    el(
      "div",
      { className: "results-footer" },
      hidden > 0
        ? el("button", {
            type: "button",
            className: "link",
            text: isDup ? `Lihat ${formatNumber(hidden)} grup lainnya` : `Lihat ${formatNumber(hidden)} file lainnya`,
            onclick: () => {
              s.expanded = true;
              render(location);
            },
          })
        : el("span"),
      el("button", {
        type: "button",
        className: "btn btn-primary btn-md",
        disabled: pickedBytes === 0 || s.deleting || s.scanning,
        text: s.deleting ? "Memindahkan…" : `Pindahkan ${formatBytes(pickedBytes, 1)} ke ${c.trash}`,
        onclick: () => deletePicked(location),
      }),
    ),
  );
}

function connectedView(location, status) {
  const s = states[location];
  return [
    header(location, status),
    stats(s.scan),
    el("div", { className: "notice" }, icon("info", 16), el("span", { text: CONFIG[location].notice })),
    results(location),
  ];
}

function setupView(location, status) {
  const c = CONFIG[location];
  const s = states[location];
  return [
    header(location, status),
    el(
      "section",
      { className: "setup" },
      el(
        "div",
        { className: "setup-intro" },
        el("h2", { className: "setup-title", text: c.setupTitle }),
        el("p", { className: "setup-text", text: c.setupIntro }),
      ),
      el(
        "ol",
        { className: "steps" },
        ...c.steps.map(([title, body], i) =>
          el(
            "li",
            { className: "step" },
            el("span", { className: "step-number", text: String(i + 1) }),
            el("span", { className: "step-title", text: title }),
            el("span", { className: "step-body", text: body }),
          ),
        ),
      ),
      el(
        "div",
        { className: "hero-actions" },
        el(
          "button",
          { type: "button", className: "btn btn-primary btn-icon", onclick: () => openExternal(c.downloadUrl) },
          c.download,
          icon("externalLink", 16),
        ),
        el(
          "button",
          { type: "button", className: "btn btn-secondary btn-icon", disabled: s.checking, onclick: () => recheck(location) },
          icon("refresh", 16),
          s.checking ? "Mengecek…" : "Cek lagi",
        ),
      ),
    ),
    el(
      "div",
      { className: "grid-2 grid-2-tight" },
      infoCard("info", c.afterTitle, c.after),
      infoCard("shieldCheck", c.skipTitle, c.skip),
    ),
  ];
}

function infoCard(iconName, title, body) {
  return el(
    "div",
    { className: "info-card card" },
    icon(iconName, 18),
    el(
      "div",
      { className: "stack-4" },
      el("span", { className: "info-card-title", text: title }),
      el("span", { className: "info-card-body", text: body }),
    ),
  );
}

function render(location) {
  const section = document.querySelector(`[data-view="${location}"]`);
  if (!section || section.hidden) return;
  const status = store.locations[location];
  const content = status?.connected ? connectedView(location, status) : setupView(location, status);
  section.replaceChildren(...content);
}

function makeView(location) {
  return {
    id: location,
    title: CONFIG[location].name,
    init() {},
    async show() {
      render(location);
      await refreshLocation(location);
      const s = states[location];
      if (store.locations[location]?.connected && !s.scan && !s.scanning) runScan(location);
    },
    render: () => render(location),
  };
}

export const googleDriveView = makeView("googleDrive");
export const icloudDriveView = makeView("icloudDrive");
