// "PC ini": which drives and what kinds of junk a PC scan includes.

import { invoke } from "../api.js";
import { $, el, icon, checkbox, toast } from "../dom.js";
import { formatGb, formatNumber, formatWhen } from "../format.js";
import { store, setApp } from "../store.js";

const TARGETS = [
  { key: "caches", label: "Cache dan file sementara", desc: "Windows, browser, dan aplikasi" },
  { key: "recycleBin", label: "Recycle Bin", desc: "Dikosongkan, bisa dipulihkan 7 hari lewat Riwayat" },
  { key: "duplicates", label: "File duplikat", desc: "Dibandingkan dari isi file, bukan dari nama" },
  { key: "largeFiles", label: "File besar", desc: () => `Lebih dari ${largeLimit()}, hanya ditampilkan` },
  { key: "oldDownloads", label: "Unduhan lama", desc: "Folder Downloads, lebih dari 90 hari" },
];

let onScan = () => {};

/** "500 MB" / "1 GB", from "Batas file besar". */
function largeLimit() {
  const mb = store.app?.settings?.largeFileMb ?? 500;
  return mb >= 1024 ? `${formatNumber(mb / 1024)} GB` : `${formatNumber(mb)} MB`;
}

const included = (letter) => !(store.app?.excludedDrives ?? []).some((d) => d.toLowerCase() === letter.toLowerCase());

async function setDriveIncluded(letter, value) {
  try {
    setApp(await invoke("set_drive_included", { letter, included: value }));
  } catch (error) {
    toast(String(error));
  }
}

async function setOption(key, value) {
  const options = { ...store.app.scanOptions, [key]: value };
  try {
    setApp(await invoke("set_scan_options", { options }));
  } catch (error) {
    toast(String(error));
  }
}

function driveType(d) {
  const kind = d.ssd === true ? "SSD" : d.ssd === false ? "HDD" : "Disk";
  return d.system ? `${kind} · drive sistem` : kind;
}

function renderDrives() {
  $("pc-drives").replaceChildren(
    ...store.drives.map((d) => {
      const on = included(d.letter);
      const fill = el("div", { className: "meter-fill" });
      fill.style.width = `${d.total ? (((d.total - d.free) / d.total) * 100).toFixed(1) : 0}%`;
      return el(
        "div",
        { className: on ? "drive-card is-on" : "drive-card" },
        el(
          "div",
          { className: "drive-top" },
          icon("hardDrive", 20),
          el(
            "div",
            { className: "drive-name" },
            el("span", { className: "drive-title", text: `${d.label} (${d.letter})` }),
            el("span", { className: "drive-type", text: driveType(d) }),
          ),
          el(
            "label",
            { className: "drive-toggle" },
            ...checkbox({ checked: on, size: 22, onChange: (v) => setDriveIncluded(d.letter, v) }),
            "Ikut discan",
          ),
        ),
        el("div", { className: "meter" }, fill),
        el(
          "div",
          { className: "drive-bottom" },
          el("span", { className: "drive-used", text: `${formatGb(d.total - d.free)} GB dari ${formatGb(d.total)} GB` }),
          el("span", { className: "drive-free", text: `${formatGb(d.free)} GB kosong` }),
        ),
      );
    }),
  );
}

function renderTargets() {
  const options = store.app?.scanOptions ?? {};
  $("pc-targets").replaceChildren(
    ...TARGETS.map((t) =>
      el(
        "label",
        { className: "target-row" },
        ...checkbox({ checked: Boolean(options[t.key]), onChange: (v) => setOption(t.key, v) }),
        el(
          "span",
          { className: "target-text" },
          el("span", { className: "target-label", text: t.label }),
          el("span", { className: "target-desc", text: typeof t.desc === "function" ? t.desc() : t.desc }),
        ),
      ),
    ),
  );
}

async function removeFolder(path) {
  try {
    setApp(await invoke("remove_skipped_folder", { path }));
  } catch (error) {
    toast(String(error));
  }
}

function renderSkipped() {
  const folders = store.app?.skippedFolders ?? [];
  $("skipped-list").replaceChildren(
    ...folders.map((path) =>
      el(
        "div",
        { className: "skipped-row" },
        icon("folderMinus", 16),
        el("span", { className: "skipped-path", text: path, title: path }),
        el(
          "button",
          { type: "button", className: "skipped-remove", "aria-label": `Hapus ${path} dari daftar`, onclick: () => removeFolder(path) },
          icon("x", 15),
        ),
      ),
    ),
  );
}

function render() {
  const count = store.drives.length;
  $("pc-sub").textContent = store.drivesCheckedAt
    ? `Terdeteksi otomatis · ${formatNumber(count)} drive · dicek ${formatWhen(store.drivesCheckedAt)}`
    : "Mendeteksi drive…";
  const anyIncluded = store.drives.some((d) => included(d.letter));
  $("pc-scan").disabled = store.scanning || !anyIncluded;
  $("pc-scan").textContent = store.scanning ? "Memindai…" : "Scan PC ini";
  renderDrives();
  renderTargets();
  renderSkipped();
}

export const pcView = {
  id: "pc",
  title: "PC ini",
  init({ scanPc }) {
    onScan = scanPc;
    $("pc-scan").addEventListener("click", () => onScan());
    $("skipped-add").addEventListener("click", async () => {
      try {
        setApp(await invoke("add_skipped_folder"));
      } catch (error) {
        toast(String(error));
      }
    });
  },
  show() {},
  render,
};
