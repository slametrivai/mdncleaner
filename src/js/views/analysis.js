// "Analisis disk": what takes up space on each drive.

import { invoke } from "../api.js";
import { $, el, icon, toast } from "../dom.js";
import { formatAgo, formatBytes, formatGb, formatNumber, formatWhen } from "../format.js";
import { store, systemDrive, checkedJunk } from "../store.js";

const TYPE_LABELS = {
  game: "Game",
  apps: "Aplikasi",
  video: "Video",
  other: "Lainnya",
  system: "Sistem Windows",
  photo: "Foto",
  docs: "Dokumen",
};

let selected = null; // drive letter
const analyses = {}; // letter -> Analysis
let running = null; // letter being analysed
let progress = 0;
let progressTimer = null;

const drive = () => store.drives.find((d) => d.letter === selected);
const junkFor = (d) => (d?.system ? checkedJunk() : 0);

function renderDrives() {
  const tabs = $("analysis-drives");
  tabs.hidden = store.drives.length < 2;
  tabs.replaceChildren(
    ...store.drives.map((d) =>
      el("button", {
        type: "button",
        role: "tab",
        className: d.letter === selected ? "segment is-active" : "segment",
        "aria-selected": String(d.letter === selected),
        text: `${d.label} (${d.letter})`,
        onclick: () => select(d.letter),
      }),
    ),
  );
}

function renderUsage() {
  const d = drive();
  if (!d?.total) return;
  const used = d.total - d.free;
  const junk = junkFor(d);
  $("usage-used").textContent = `${formatGb(used)} GB `;
  $("usage-total").textContent = `terpakai dari ${formatGb(d.total)} GB`;
  $("usage-free").textContent = `${formatGb(d.free)} GB kosong${junk > 0 ? " · " : ""}`;
  $("usage-junk").textContent = junk > 0 ? `${formatBytes(junk)} sampah` : "";
  $("usage-seg-used").style.width = `${(((used - junk) / d.total) * 100).toFixed(1)}%`;
  $("usage-seg-junk").hidden = junk === 0;
}

function placeholder(text) {
  return el("p", { className: "list-empty list-empty-flush", text });
}

function renderTypes(analysis) {
  const list = $("type-list");
  if (!analysis) {
    list.replaceChildren(placeholder(running ? "Menghitung…" : "Belum dipindai."));
    return;
  }
  const rows = analysis.types.filter((t) => t.bytes > 0).map((t) => ({ label: TYPE_LABELS[t.kind] ?? t.kind, bytes: t.bytes }));
  const junk = junkFor(drive());
  if (junk > 0) rows.push({ label: "Sampah", bytes: junk, junk: true });
  const max = Math.max(1, ...rows.map((r) => r.bytes));
  list.replaceChildren(
    ...rows.map((r) => {
      const fill = el("div", { className: r.junk ? "type-fill is-junk" : "type-fill" });
      fill.style.width = `${Math.max(2, (r.bytes / max) * 100).toFixed(1)}%`;
      return el(
        "div",
        { className: r.junk ? "type-row is-junk" : "type-row" },
        el("span", { className: "type-label", text: r.label }),
        el("div", { className: "type-track" }, fill),
        el("span", { className: "type-value", text: formatBytes(r.bytes, 1) }),
      );
    }),
  );
}

function renderFolders(analysis) {
  const list = $("folder-list");
  if (!analysis) {
    list.replaceChildren(placeholder(running ? "Menghitung…" : "Belum dipindai."));
    return;
  }
  list.replaceChildren(
    ...analysis.folders.map((f) =>
      el(
        "button",
        { type: "button", className: "folder-row", title: "Buka di File Explorer", onclick: () => reveal(f.path) },
        icon("folderOpen", 16),
        el("span", { className: "folder-path", text: f.path }),
        el("span", { className: "folder-size", text: formatBytes(f.bytes, 1) }),
      ),
    ),
  );
}

function renderLarge(analysis) {
  const card = $("large-card");
  card.hidden = store.app?.scanOptions?.largeFiles === false;
  const list = $("large-list");
  if (!analysis) {
    list.replaceChildren(placeholder(running ? "Mencari file besar…" : "Belum dipindai."));
    return;
  }
  if (analysis.largeFiles.length === 0) {
    list.replaceChildren(placeholder("Tidak ada file di atas 500 MB yang lama tidak dibuka."));
    return;
  }
  list.replaceChildren(
    ...analysis.largeFiles.map((f) =>
      el(
        "div",
        { className: "large-row" },
        el(
          "div",
          { className: "large-name" },
          el("span", { className: "large-file", text: f.name }),
          el("span", { className: "large-folder", text: f.folder }),
        ),
        el("span", { className: "large-ago", text: `Dibuka ${formatAgo(f.lastUsedMs)}` }),
        el("span", { className: "large-size", text: formatBytes(f.bytes, 1) }),
        el("button", { type: "button", className: "btn-small", text: "Buka lokasi", onclick: () => reveal(f.path) }),
      ),
    ),
  );
}

async function reveal(path) {
  try {
    await invoke("reveal", { path });
  } catch (error) {
    toast(`Lokasi tidak bisa dibuka: ${error}`);
  }
}

function render() {
  if (!selected || !drive()) selected = systemDrive()?.letter ?? null;
  const analysis = analyses[selected];
  const sub = $("analysis-sub");
  if (running === selected) {
    sub.textContent = `Memindai ${formatNumber(progress)} file…`;
  } else if (analysis) {
    sub.textContent = `Lihat apa yang paling banyak makan tempat. Dipindai ${formatWhen(analysis.atMs)}.`;
  } else {
    sub.textContent = "Lihat apa yang paling banyak makan tempat.";
  }
  $("analysis-rescan").disabled = running !== null;
  renderDrives();
  renderUsage();
  renderTypes(analysis);
  renderFolders(analysis);
  renderLarge(analysis);
}

async function analyze(letter) {
  if (running) return;
  running = letter;
  progress = 0;
  progressTimer = setInterval(async () => {
    progress = await invoke("analysis_progress").catch(() => progress);
    render();
  }, 400);
  render();
  try {
    analyses[letter] = await invoke("analyze", { letter });
  } catch (error) {
    toast(`Analisis gagal: ${error}`);
  }
  clearInterval(progressTimer);
  running = null;
  render();
}

async function select(letter) {
  selected = letter;
  if (!analyses[letter]) {
    const last = await invoke("last_analysis", { letter }).catch(() => null);
    if (last) analyses[letter] = last;
  }
  render();
  if (!analyses[letter]) analyze(letter);
}

export const analysisView = {
  id: "analysis",
  title: "Analisis disk",
  init() {
    $("analysis-rescan").addEventListener("click", () => selected && analyze(selected));
  },
  show() {
    const letter = selected ?? systemDrive()?.letter;
    if (letter) select(letter);
  },
  render,
};
