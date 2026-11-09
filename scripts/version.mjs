// Sets the app version everywhere it is declared:
//   npm run set-version 1.1.0

import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("Pakai: npm run set-version <major.minor.patch>, contoh: npm run set-version 1.1.0");
  process.exit(1);
}

function update(path, transform) {
  const before = readFileSync(path, "utf8");
  const after = transform(before);
  if (after === before) throw new Error(`Versi tidak ditemukan di ${path}`);
  writeFileSync(path, after);
  console.log(`  ${path}`);
}

const jsonVersion = (text) => text.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`);

update("package.json", jsonVersion);
update("src-tauri/tauri.conf.json", jsonVersion);
update("src-tauri/Cargo.toml", (text) => text.replace(/^(version\s*=\s*")[^"]+(")/m, `$1${version}$2`));
console.log(`Versi sekarang ${version}. Tulis perubahannya di release-notes.md, lalu commit dan tag v${version}.`);
