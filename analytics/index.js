// Firebase Analytics for MDN Cleaner, bundled by esbuild into
// src/vendor/analytics.js (see `npm run bundle`).
//
// Only anonymous usage is sent: which screens are opened and how much a clean
// freed. File names, paths and folder contents never leave the PC. Firebase
// gives each install its own random ID, which is what counts as one "user".

import { initializeApp } from "firebase/app";
import { getAnalytics, isSupported, logEvent, setUserProperties } from "firebase/analytics";

// Filled in from .env at bundle time (analytics/build.mjs). A Firebase web
// config identifies the project and ships inside the app; it is not a secret.
const firebaseConfig = {
  apiKey: process.env.FIREBASE_API_KEY,
  authDomain: process.env.FIREBASE_AUTH_DOMAIN,
  projectId: process.env.FIREBASE_PROJECT_ID,
  storageBucket: process.env.FIREBASE_STORAGE_BUCKET,
  messagingSenderId: process.env.FIREBASE_MESSAGING_SENDER_ID,
  appId: process.env.FIREBASE_APP_ID,
  measurementId: process.env.FIREBASE_MEASUREMENT_ID,
};

let analytics = null;

/** Starts analytics. Never throws: offline or blocked, the app just runs. */
export async function initAnalytics({ version } = {}) {
  try {
    if (!firebaseConfig.apiKey || !firebaseConfig.measurementId) return;
    if (!(await isSupported())) return;
    analytics = getAnalytics(initializeApp(firebaseConfig));
    setUserProperties(analytics, { app_version: version ?? "unknown" });
  } catch (error) {
    console.warn("analytics off:", error);
  }
}

/** Logs one event. A no-op until analytics has started. */
export function track(name, params = {}) {
  if (!analytics) return;
  try {
    logEvent(analytics, name, params);
  } catch (error) {
    console.warn("analytics event dropped:", error);
  }
}
