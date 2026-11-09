import { readFileSync } from "node:fs";
import { ensureInstalledInit } from "../paths";

/** The flagless polyfill header (init.js). Seeded on first use. */
export function polyfillHeader(): string {
  return readFileSync(ensureInstalledInit(), "utf8");
}
