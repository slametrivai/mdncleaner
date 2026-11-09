// "Tentang": app info, the maker's website and version.

import { invoke } from "../api.js";
import { $, toast } from "../dom.js";

const SITE = "slametrivai.my.id";
const SITE_URL = `https://${SITE}`;

async function copySite() {
  try {
    await navigator.clipboard.writeText(SITE_URL);
    toast("Alamat website disalin.");
  } catch {
    toast(`Salin manual: ${SITE_URL}`);
  }
}

async function openSite() {
  try {
    await invoke("open_external", { url: SITE_URL });
  } catch (error) {
    toast(`Website tidak bisa dibuka: ${error}`);
  }
}

export function initAbout(version) {
  const dialog = $("about");
  const close = () => dialog.close();
  if (version) {
    $("about-version").textContent = version;
    $("about-version-chip").textContent = `Versi ${version} ·`;
  }
  $("about-open").addEventListener("click", () => dialog.showModal());
  $("about-close").addEventListener("click", close);
  $("about-done").addEventListener("click", close);
  $("about-copy").addEventListener("click", copySite);
  $("about-site-open").addEventListener("click", openSite);
  $("about-site").addEventListener("click", openSite);
  // A click on the backdrop lands on the dialog element itself.
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) close();
  });
}
