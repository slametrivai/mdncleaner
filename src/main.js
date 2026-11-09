// App shell: window controls, sidebar navigation and shared panels.

import { invoke, appWindow, getVersion } from "./js/api.js";
import { $, icon, toast } from "./js/dom.js";
import { formatGb } from "./js/format.js";
import { store, subscribe, setApp, refreshDrives, refreshLocation, systemDrive } from "./js/store.js";
import { cleanerView, scan as scanPc } from "./js/views/cleaner.js";
import { analysisView } from "./js/views/analysis.js";
import { historyView } from "./js/views/history.js";
import { pcView } from "./js/views/pc.js";
import { googleDriveView, icloudDriveView } from "./js/views/cloud.js";
import { initAbout } from "./js/views/about.js";
import { settingsView } from "./js/views/settings.js";
import { startAnalytics, track } from "./js/analytics.js";

const VIEWS = [cleanerView, analysisView, historyView, pcView, googleDriveView, icloudDriveView, settingsView];
let current = null;

function navigate(id) {
  const view = VIEWS.find((v) => v.id === id);
  if (!view) return;
  current = view;
  for (const section of document.querySelectorAll("[data-view]")) {
    section.hidden = section.dataset.view !== id;
  }
  for (const item of document.querySelectorAll("[data-nav]")) {
    const active = item.dataset.nav === id;
    item.classList.toggle("is-active", active);
    if (active) item.setAttribute("aria-current", "page");
    else item.removeAttribute("aria-current");
  }
  $("titlebar-title").textContent = view.title;
  track("screen_view", { firebase_screen: view.title, firebase_screen_class: view.id });
  $("content").scrollTop = 0;
  view.render();
  view.show();
}

function renderDiskCard() {
  const disk = systemDrive();
  if (!disk?.total) return;
  const used = disk.total - disk.free;
  $("disk-drive").textContent = `Drive ${disk.letter}`;
  $("disk-usage").textContent = `${formatGb(used)} dari ${formatGb(disk.total)} GB`;
  $("disk-fill").style.width = `${Math.min(100, (used / disk.total) * 100).toFixed(1)}%`;
  $("disk-free").textContent = `${formatGb(disk.free)} GB masih kosong`;
}

function renderLocations() {
  const connected = {
    pc: store.drives.length > 0,
    googleDrive: store.locations.googleDrive?.connected,
    icloudDrive: store.locations.icloudDrive?.connected,
  };
  for (const node of document.querySelectorAll("[data-location]")) {
    const key = node.dataset.location;
    const known = key === "pc" || store.locations[key] != null;
    const ok = Boolean(connected[key]);
    node.classList.toggle("is-missing", known && !ok);
    node.replaceChildren(!known ? "" : ok ? icon("check", 16) : "Belum");
    node.setAttribute("aria-label", ok ? "Terhubung" : "Belum terhubung");
  }
}

function applyTheme() {
  document.documentElement.dataset.theme = store.app?.settings?.theme ?? "light";
}

async function checkUpdates() {
  try {
    await invoke("check_updates");
    toast("MDN Cleaner sudah versi terbaru.");
  } catch (error) {
    toast(String(error));
  }
}

function bindShell() {
  $("win-minimize").addEventListener("click", () => appWindow.minimize());
  $("win-maximize").addEventListener("click", () => appWindow.toggleMaximize());
  // With "Jalankan saat Windows menyala" on, the backend turns this into "hide to tray".
  $("win-close").addEventListener("click", () => appWindow.close());
  $("update-check").addEventListener("click", checkUpdates);
  for (const item of document.querySelectorAll("[data-nav]")) {
    item.addEventListener("click", () => navigate(item.dataset.nav));
  }
}

async function init() {
  bindShell();
  for (const view of VIEWS) {
    view.init({
      scanPc: () => {
        navigate("cleaner");
        scanPc();
      },
    });
  }

  subscribe(() => {
    applyTheme();
    renderDiskCard();
    renderLocations();
    current?.render();
  });

  const [app, version] = await Promise.all([
    invoke("get_state").catch(() => null),
    getVersion().catch(() => null),
    refreshDrives(),
  ]);
  if (version) $("app-version").textContent = `Versi ${version}`;
  initAbout(version);
  startAnalytics(version);
  setApp(app);

  navigate("cleaner");
  if (store.app?.settings?.checkCloudOnStart !== false) {
    refreshLocation("googleDrive");
    refreshLocation("icloudDrive");
  }
}

init();
