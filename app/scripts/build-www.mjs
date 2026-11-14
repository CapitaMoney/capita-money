// SPDX-License-Identifier: AGPL-3.0-only
// Copies what the app ships into www/ – for the Android shell (Capacitor,
// webDir) and for GitHub Pages. Tests, scripts, node_modules and the shell
// itself stay out. Run: npm run web
import { cpSync, rmSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(here, "www");
const SHIP = ["index.html", "app.css", "app.js", "logic.js", "sw.js", "manifest.webmanifest", "vendor", "icons", "fonts"];
const OPTIONAL = ["chain", "native"];
rmSync(out, { recursive: true, force: true });
mkdirSync(out);
for (const f of SHIP) {
  const from = join(here, f);
  if (!existsSync(from)) throw new Error(`missing ${f}`);
  cpSync(from, join(out, f), { recursive: true });
}
// The site's statement that the Android shell may use its passkeys (ADR 0023).
cpSync(join(here, "well-known"), join(out, ".well-known"), { recursive: true });
for (const f of OPTIONAL) if (existsSync(join(here, f))) cpSync(join(here, f), join(out, f), { recursive: true });
console.log(`www/: ${SHIP.length} entries and what exists of ${OPTIONAL.join(", ")}`);
