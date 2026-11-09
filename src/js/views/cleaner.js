// "Pembersih": the junk found on this PC, ready to clean.

import { invoke } from "../api.js";
import { $, el, checkbox, toast } from "../dom.js";
import { formatBytes, formatDate, formatGb, formatNumber } from "../format.js";
import { track, toMb } from "../analytics.js";
import { store, notify, setApp, refreshDrives, systemDrive, checkedJunk, confirmDelete } from "../store.js";

const GROUPS = [
  { id: "windows", label: "Windows" },
  { id: "browser", label: "Browser" },
  { id: "apps", label: "Aplikasi" },
  { id: "files", label: "File pribadi" },
];

const RECYCLE_NOTE = "File dikirim ke Recycle Bin lebih dulu dan masih bisa dipulihkan selama 7 hari.";
const PERMANENT_NOTE = "Recycle Bin dimatikan di Pengaturan, jadi file langsung dihapus permanen.";

const usesRecycleBin = () => store.app?.settings?.useRecycleBin !== false;
const defaultNote = () => (usesRecycleBin() ? RECYCLE_NOTE : PERMANENT_NOTE);

let cleaning = false;
let note = null; // a result message; null shows the default note
let scannedOnce = false;

const isChecked = (item) => store.app?.selection?.[item.id] ?? item.defaultChecked;
const sum = (items) => items.reduce((total, item) => total + item.bytes, 0);

function selectedByGroup() {
  return GROUPS.map((group) => {
    const items = store.items.filter((item) => item.group === group.id);
    return { ...group, present: items.length > 0, bytes: sum(items.filter(isChecked)) };
  }).filter((group) => group.present);
}

function renderHero() {
  const title = $("hero-title");
  const total = checkedJunk();
  const busy = store.scanning || cleaning;

  if (cleaning) {
    title.textContent = "Membersihkan sampah…";
  } else if (store.scanning) {
    title.textContent = "Menganalisis sampah…";
  } else if (total > 0) {
    title.replaceChildren("Ada ", el("span", { className: "accent", text: formatBytes(total) }), " sampah siap dibersihkan");
  } else if (sum(store.items) === 0) {
    title.textContent = "PC kamu sudah bersih";
  } else {
    title.textContent = "Pilih sampah yang mau dibersihkan";
  }
  $("hero-note").textContent = note ?? defaultNote();

  const cleanButton = $("btn-clean");
  cleanButton.disabled = busy || total === 0;
  cleanButton.textContent = cleaning ? "Membersihkan…" : "Bersihkan sekarang";

  const rescanButton = $("btn-rescan");
  rescanButton.disabled = busy;
  rescanButton.textContent = store.scanning ? "Menganalisis…" : "Analisis ulang";

  const groups = selectedByGroup();
  const withBytes = groups.filter((g) => g.bytes > 0);
  $("breakdown-bar").replaceChildren(
    ...(withBytes.length
      ? withBytes.map((g) => {
          const seg = el("div", { className: `breakdown-seg is-${g.id}` });
          seg.style.flexGrow = String(g.bytes);
          return seg;
        })
      : [el("div", { className: "breakdown-seg is-empty" })]),
  );
  $("legend").replaceChildren(
    ...groups.map((g) =>
      el(
        "li",
        {},
        el("span", { className: `legend-dot is-${g.id}`, "aria-hidden": "true" }),
        el("span", { className: "legend-label", text: g.label }),
        el("span", { className: "legend-value", text: formatBytes(g.bytes) }),
      ),
    ),
  );

  const disk = systemDrive();
  $("breakdown-note").textContent = disk?.total
    ? `Drive ${disk.letter} jadi ${formatGb(disk.free + total)} GB kosong dari ${formatGb(disk.total)} GB`
    : " ";
}

async function toggleItem(id, checked) {
  store.app.selection = { ...store.app.selection, [id]: checked };
  notify();
  try {
    await invoke("set_selection", { id, checked });
  } catch (error) {
    console.error("set_selection", error);
  }
}

function renderList() {
  const list = $("category-list");
  list.setAttribute("aria-busy", String(store.scanning));

  if (store.items.length === 0) {
    const text = store.scanning ? "Menganalisis…" : "Tidak ada lokasi sampah yang ditemukan.";
    list.replaceChildren(el("p", { className: "list-empty", text }));
    return;
  }

  const sections = GROUPS.map((group) => {
    const items = store.items.filter((item) => item.group === group.id);
    if (items.length === 0) return null;
    const headingId = `group-${group.id}`;
    return el(
      "section",
      { "aria-labelledby": headingId },
      el(
        "div",
        { className: "group-header" },
        el("h2", { className: "group-title", id: headingId, text: group.label }),
        el("span", { className: "group-meta", text: `${formatBytes(sum(items.filter(isChecked)))} dipilih` }),
      ),
      ...items.map((item) =>
        el(
          "label",
          { className: "row" },
          ...checkbox({
            checked: isChecked(item),
            disabled: cleaning,
            onChange: (checked) => toggleItem(item.id, checked),
          }),
          el(
            "span",
            { className: "row-label" },
            item.label,
            item.note ? el("span", { className: "row-note", text: item.note }) : null,
          ),
          el("span", { className: "row-size", text: formatBytes(item.bytes) }),
        ),
      ),
    );
  });
  list.replaceChildren(...sections.filter(Boolean));
}

function renderLastClean() {
  const last = store.app?.history?.find((e) => e.kind === "manual" || e.kind === "auto");
  if (!last) return;
  $("last-clean-date").textContent = formatDate(last.atMs);
  $("last-clean-detail").textContent = `${formatBytes(last.bytes, 1)} dibebaskan dari ${formatNumber(last.files)} file`;
}

function render() {
  renderHero();
  renderList();
  renderLastClean();
  $("auto-clean-switch").setAttribute("aria-checked", String(Boolean(store.app?.autoClean)));
}

export async function scan() {
  if (store.scanning) return;
  scannedOnce = true;
  store.scanning = true;
  notify();
  try {
    store.items = await invoke("scan");
  } catch (error) {
    note = `Analisis gagal: ${error}`;
  }
  store.scanning = false;
  notify();
}

async function clean() {
  const picked = store.items.filter(isChecked);
  const ids = picked.map((item) => item.id);
  if (ids.length === 0) return;
  const total = formatBytes(checkedJunk());
  const where = usesRecycleBin()
    ? "File dipindah ke Recycle Bin dan bisa dipulihkan 7 hari lewat Riwayat."
    : "File langsung dihapus permanen dan tidak bisa dipulihkan.";
  const list = picked.map((item) => `• ${item.label} (${formatBytes(item.bytes)})`).join("\n");
  if (!(await confirmDelete(`Bersihkan ${total}?`, `${list}\n\n${where}`, "Bersihkan"))) return;
  cleaning = true;
  notify();
  try {
    const { report, state } = await invoke("clean", { ids });
    setApp(state);
    track("clean_completed", { freed_mb: toMb(report.bytes), items: ids.length });
    note =
      report.skipped > 0
        ? `${formatBytes(report.bytes)} dibersihkan. ${formatNumber(report.skipped)} item dilewati karena sedang dipakai program lain atau butuh izin admin.`
        : `${formatBytes(report.bytes)} dibersihkan. ${defaultNote()}`;
  } catch (error) {
    note = `Pembersihan gagal: ${error}`;
  }
  cleaning = false;
  await refreshDrives();
  await scan();
}

async function toggleAutoClean() {
  const toggle = $("auto-clean-switch");
  const errorLine = $("auto-clean-error");
  const enabled = toggle.getAttribute("aria-checked") !== "true";
  toggle.disabled = true;
  errorLine.hidden = true;
  try {
    setApp(await invoke("set_auto_clean", { enabled }));
    track("auto_clean_set", { enabled });
    toast(enabled ? "Bersih otomatis aktif setiap Minggu, 20.00." : "Bersih otomatis dimatikan.");
  } catch (error) {
    errorLine.textContent = `Jadwal tidak bisa diubah: ${error}`;
    errorLine.hidden = false;
  }
  toggle.disabled = false;
  render();
}

export const cleanerView = {
  id: "cleaner",
  title: "Pembersih PC",
  init() {
    $("btn-clean").addEventListener("click", clean);
    $("btn-rescan").addEventListener("click", () => {
      note = null;
      scan();
    });
    $("auto-clean-switch").addEventListener("click", toggleAutoClean);
  },
  show() {
    if (!scannedOnce) scan();
  },
  render,
};
