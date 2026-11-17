#!/usr/bin/env node
// Decide what a submission scan result means. Runs in the privileged publish
// job, but the result file itself is untrusted: everything is validated
// before it can cause a branch, a commit, or a comment.
//
//   node tools/submit/decide.mjs --result /tmp/result.json --issue 123
//        [--index index/index.json] [--out /tmp/decision]
//
// Writes <out>.json {action, entry, reason} and <out>.md (the issue comment).
// action is "pr" (clean verified result: open a listing PR) or "review"
// (anything else: label needs-review, no files touched).

import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { validateResult } from "./validate.mjs";

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};

const raw = readFileSync(resolve(flag("result", "")), "utf8");
const issue = Number(flag("issue", "0"));
const out = flag("out", "/tmp/decision");
let record;
try {
  record = JSON.parse(raw);
} catch {
  record = null;
}

const decide = (action, entry, reason, comment) => {
  writeFileSync(`${out}.json`, `${JSON.stringify({ action, entry, reason, issue }, null, 2)}\n`);
  writeFileSync(`${out}.md`, `${comment}\n`);
  console.log(`${action}: ${reason}`);
};

const problems = (rec) => {
  const list = [];
  for (const t of rec.tools) {
    if (!t.description.trim()) list.push(`\`${t.name}\`: no description`);
    for (const [field, schema] of Object.entries(t.input_schema?.properties ?? {})) {
      if (schema && typeof schema === "object" && !schema.description) list.push(`\`${t.name}.${field}\`: no field description`);
    }
    for (const f of t.flags ?? []) list.push(`\`${t.name}\`: description flagged (${f})`);
  }
  return list;
};

if (!record || typeof record !== "object") {
  decide("review", "", "the scan produced no readable result", "The automated check could not read this site. A maintainer will take a look.");
  process.exit(0);
}

// The entry must be re-derived from the record's own pages, never trusted
// from the issue.
const entry = record.pages_checked?.[0]
  ? record.pages_checked[0]
      .replace(/^https:\/\//, "")
      .replace(/\/$/, "")
      .replace(/^www\./, "")
  : "";
const v = validateResult(record, { entry: entry.split("/")[0] });
if (!v.ok) {
  decide("review", entry, `validation failed: ${v.reason}`, "The automated check finished, but its result did not validate. A maintainer will take a look.");
  process.exit(0);
}

const firstTool = record.tools[0]?.name.replace(/[_.-]+/g, " ") ?? "";
if (record.status === "verified" && record.tools.length > 0) {
  const probs = problems(record);
  const fix = probs.length
    ? `\nA few things you (or your coding agent) can fix to rank higher:\n${probs.slice(0, 10).map((p) => `- ${p}`).join("\n")}\n`
    : "\nDescriptions and schemas all check out.\n";
  decide(
    "pr",
    entry,
    `${record.tools.length} tools verified`,
    [
      `Checked ${record.pages_checked.length} ${record.pages_checked.length === 1 ? "page" : "pages"} just now: **${record.tools.length} WebMCP ${record.tools.length === 1 ? "tool" : "tools"} found**.`,
      "",
      `Tool${record.tools.length === 1 ? "" : "s"}: ${record.tools.map((t) => `\`${t.name}\``).join(", ")}`,
      fix,
      `Once merged, agents will find you with: \`agent-webmcp find "${record.host} ${firstTool}"\``,
      "",
      "A listing PR is opening automatically; a maintainer merges it to go live.",
    ].join("\n"),
  );
  process.exit(0);
}

const why =
  record.status === "blocked"
    ? "A bot check stopped the browser, so its pages could not be seen. The check was not solved and never is."
    : record.status === "error"
      ? `The pages did not finish loading (${(record.note ?? "see the log").slice(0, 120)}).`
      : "Its pages loaded and none registered a WebMCP tool. Tools that appear only after logging in or interacting are not indexed yet.";
decide("review", entry, `no tools listed (${record.status})`, `${why} A maintainer will confirm and close this out.`);
