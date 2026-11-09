// Bundles analytics/index.js into src/vendor/analytics.js, filling in the
// Firebase config from .env (or the environment, for CI).

import { build } from "esbuild";
import { existsSync, readFileSync } from "node:fs";

const KEYS = [
  "FIREBASE_API_KEY",
  "FIREBASE_AUTH_DOMAIN",
  "FIREBASE_PROJECT_ID",
  "FIREBASE_STORAGE_BUCKET",
  "FIREBASE_MESSAGING_SENDER_ID",
  "FIREBASE_APP_ID",
  "FIREBASE_MEASUREMENT_ID",
];

function readEnvFile(path) {
  if (!existsSync(path)) return {};
  const values = {};
  for (const line of readFileSync(path, "utf8").split(/\r?\n/)) {
    const match = line.match(/^\s*([A-Z0-9_]+)\s*=\s*(.*?)\s*$/);
    if (match) values[match[1]] = match[2].replace(/^["']|["']$/g, "");
  }
  return values;
}

const fromFile = readEnvFile(".env");
const config = Object.fromEntries(KEYS.map((key) => [key, process.env[key] ?? fromFile[key] ?? ""]));

const missing = KEYS.filter((key) => !config[key]);
if (missing.length) {
  console.warn(`analytics: ${missing.join(", ")} kosong, analytics akan mati di build ini.`);
}

await build({
  entryPoints: ["analytics/index.js"],
  bundle: true,
  format: "esm",
  minify: true,
  target: "es2020",
  outfile: "src/vendor/analytics.js",
  define: Object.fromEntries(KEYS.map((key) => [`process.env.${key}`, JSON.stringify(config[key])])),
  logLevel: "info",
});
