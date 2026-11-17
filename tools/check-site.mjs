#!/usr/bin/env node
// Check one site and report what its pages register, as JSON.
//
//   node tools/check-site.mjs <url> [--pages 4] [--engines agent-browser]
//
// This is what a "list my site" request runs, and what a site owner can run
// themselves before asking. It reads only: no tool is invoked, no form is
// submitted, no bot check is solved. Exit code is 0 when at least one tool was
// seen, 2 when the site loaded with none, 1 when it could not be checked.
// A missing engine binary yields an `error` record on stdout (exit 1), never
// an empty stdout: the publish job rejects empty output during validation.

import { checkSite } from "./build-index/build.mjs";

const args = process.argv.slice(2);
const url = args.find((a) => !a.startsWith("--"));
if (!url) {
  console.error("usage: node tools/check-site.mjs <url> [--pages 4] [--engines agent-browser]");
  process.exit(1);
}

const flag = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};

const entry = url.replace(/^https?:\/\//, "").replace(/\/$/, "");
const pages = Number(flag("pages", 4));
const engines = String(flag("engines", "agent-browser"))
  .split(",")
  .map((e) => e.trim())
  .filter(Boolean);
const site = await checkSite(entry, { pages, engines });
process.stdout.write(`${JSON.stringify(site, null, 2)}\n`);

const tools = site.tools.length;
const problems = [];
for (const t of site.tools) {
  if (!t.description.trim()) problems.push(`${t.name}: no description`);
  const props = t.input_schema?.properties ?? {};
  for (const [field, schema] of Object.entries(props)) {
    if (!schema || typeof schema !== "object") continue;
    if (!schema.description) problems.push(`${t.name}.${field}: no description`);
  }
  if (t.flags?.length) problems.push(`${t.name}: description flagged (${t.flags.join(", ")})`);
}

console.error(
  `${site.host}: ${site.status}, ${tools} ${tools === 1 ? "tool" : "tools"}, ${site.pages_checked.length} pages checked`,
);
if (problems.length) {
  console.error(`${problems.length} thing${problems.length === 1 ? "" : "s"} a site owner can fix:`);
  for (const p of problems.slice(0, 20)) console.error(`  - ${p}`);
}
if (!tools) {
  console.error(
    site.status === "blocked"
      ? "a bot check stopped the browser; it was not solved"
      : "no WebMCP tools were registered at load",
  );
}
process.exit(tools > 0 ? 0 : 2);
