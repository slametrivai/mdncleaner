// Thin wrapper over the Tauri globals (withGlobalTauri: true).

export const { invoke } = window.__TAURI__.core;
export const appWindow = window.__TAURI__.window.getCurrentWindow();
export const getVersion = () => window.__TAURI__.app.getVersion();
