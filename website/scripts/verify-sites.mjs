// verify-sites: build-time gate for sites.json. v1 record shape:
// origin (https URL), slug (raw domain), description 1-140 chars,
// invoke {transport, endpoint, example} (the calling contract),
// proof {verified date, ref evidence note}.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const fails = [];
const fail = (slug, msg) => fails.push(`${slug}: ${msg}`);

const TRANSPORTS = ["deep-link", "mcp", "webmcp", "api", "dom"];

const file = JSON.parse(readFileSync(join(root, "sites.json"), "utf8"));
if (!Array.isArray(file.sites)) {
  console.error("FAIL sites.json: sites is not an array");
  process.exit(1);
}
const seen = new Set();
const foldedSeen = new Set();
for (const s of file.sites) {
  const tag = s.slug ?? s.origin ?? "?";
  if (typeof s.origin !== "string" || !/^https:\/\//.test(s.origin)) fail(tag, "origin must be an https URL");
  const host = s.origin.replace(/^https?:\/\//, "").split("/")[0] ?? "";
  if (host !== host.toLowerCase() || !/^[a-z0-9.-]+\.[a-z]{2,}$/.test(host)) fail(tag, "origin host must be a canonical lowercase domain");
  if (s.slug !== host) fail(tag, "slug must be the raw domain");
  if (seen.has(host)) fail(tag, "duplicate domain");
  seen.add(host);
  const folded = host.startsWith("www.") ? host.slice(4) : host;
  if (foldedSeen.has(folded)) fail(tag, "www/apex duplicate — one row per site");
  foldedSeen.add(folded);
  if (typeof s.description !== "string" || s.description === "" || s.description.length > 140)
    fail(tag, "description must be 1-140 chars");
  if (typeof s.name !== "string" || s.name === "" || s.name.length > 40)
    fail(tag, "name must be a 1-40 char display name");
  if (typeof s.platform !== "string") fail(tag, "platform must be a string (\"\" when unknown)");
  else if (s.platform !== "" && !/^[a-z0-9.-]+$/.test(s.platform)) fail(tag, "platform must be a lowercase id");
  if (typeof s.skill !== "string" || s.skill === "" || s.skill.length > 4000)
    fail(tag, "skill must be a 1-4000 char get_agent payload");
  if (typeof s.invoke !== "object" || s.invoke === null) fail(tag, "invoke contract is required");
  else {
    if (!TRANSPORTS.includes(s.invoke.transport)) fail(tag, `invoke.transport must be one of ${TRANSPORTS.join("|")}`);
    if (typeof s.invoke.endpoint !== "string" || s.invoke.endpoint === "") fail(tag, "invoke.endpoint must be a non-empty string");
    if (typeof s.invoke.example !== "string" || s.invoke.example === "") fail(tag, "invoke.example must be a non-empty string");
  }
  if (typeof s.proof?.verified !== "string" || Number.isNaN(Date.parse(s.proof.verified)))
    fail(tag, "proof.verified must be a date");
  if (typeof s.proof?.ref !== "string" || s.proof.ref === "") fail(tag, "proof.ref must be a non-empty evidence note");
  if (!Array.isArray(s.tools)) fail(tag, "tools must be an array (possibly empty)");
  else
    for (const t of s.tools) {
      if (typeof t !== "object" || t === null || typeof t.name !== "string" || t.name === "")
        fail(tag, "tools[].name must be a non-empty string");
      else if (typeof t.params !== "string") fail(tag, `tools[${t.name}].params must be a string`);
    }
}
if (fails.length > 0) {
  console.error(`FAIL verify-sites:\n${fails.map((f) => ` - ${f}`).join("\n")}`);
  process.exit(1);
}
console.log(`verify-sites: ${file.sites.length} site(s) ok`);
