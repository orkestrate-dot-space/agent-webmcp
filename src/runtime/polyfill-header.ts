import { readFileSync } from "node:fs";
import { installedInitPath } from "../paths";

/** The flagless polyfill header (init.js). Single source: the installed copy. */
export function polyfillHeader(): string {
  return readFileSync(installedInitPath(), "utf8");
}
