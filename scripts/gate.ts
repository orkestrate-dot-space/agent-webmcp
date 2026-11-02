// gate: five deterministic checks, no browser, no network.
// typecheck + unit tests + skill shape + schema-object + desc-budget.
const fail = (msg: string): never => { console.error(`FAIL ${msg}`); process.exit(1); };
const pass = (msg: string): void => console.log(`PASS ${msg}`);

const run = async (cmd: Array<string>): Promise<{ code: number; out: string }> => {
  const proc = Bun.spawn(cmd, { stdout: "pipe", stderr: "pipe", cwd: import.meta.dir + "/.." });
  const [out, err, code] = await Promise.all([new Response(proc.stdout as ReadableStream).text(), new Response(proc.stderr as ReadableStream).text(), proc.exited]);
  return { code, out: out + err };
};

// 1. typecheck
{
  const r = await run(["bunx", "tsc", "--noEmit"]);
  if (r.code !== 0) { console.error(r.out); fail("typecheck"); }
  pass("typecheck");
}
// 2. unit tests
{
  const r = await run(["bun", "test", "src"]);
  if (r.code !== 0) { console.error(r.out); fail("unit tests"); }
  pass("unit tests");
}
// 3. skill shape: SKILL.md exists, frontmatter name matches dirname
{
  const { readFileSync, existsSync } = await import("node:fs");
  const root = import.meta.dir + "/../skills/agent-webmcp";
  if (!existsSync(`${root}/SKILL.md`)) fail("skill-shape (missing SKILL.md)");
  const core = readFileSync(`${root}/SKILL.md`, "utf8");
  const name = /^name:\s*(\S+)/m.exec(core)?.[1];
  if (name !== "agent-webmcp") fail("skill-shape (core name mismatch)");
  pass("skill-shape");
}
// 4. schema-object: every served inputSchema is a top-level object
{
  const { VERBS } = await import("../src/server.ts");
  for (const v of VERBS as Array<{ name: string; inputSchema: unknown }>) {
    const s = v.inputSchema as { type?: unknown } | null;
    if (typeof s !== "object" || s === null || (s as { type?: unknown }).type !== "object") fail(`schema-object (${v.name})`);
  }
  pass("schema-object");
}
// 5. desc-budget: every served description ≤ 1500 chars
{
  const { VERBS } = await import("../src/server.ts");
  for (const v of VERBS as Array<{ name: string; description: string; inputSchema: { properties?: Record<string, { description?: string }> } }>) {
    if (v.description.length > 1500) fail(`desc-budget (${v.name} description)`);
    for (const [k, p] of Object.entries(v.inputSchema.properties ?? {})) {
      if ((p.description ?? "").length > 1500) fail(`desc-budget (${v.name}.${k})`);
    }
  }
  pass("desc-budget");
}
// 6. base-budget: the fixed per-turn cost stays small — skill text +
// served descriptions ≤ 8000 chars (~2k tokens). Growth fails loud.
{
  const { readFileSync } = await import("node:fs");
  const { VERBS } = await import("../src/server.ts");
  let total = readFileSync(import.meta.dir + "/../skills/agent-webmcp/SKILL.md", "utf8").length;
  for (const v of VERBS as Array<{ description: string; inputSchema: { properties?: Record<string, { description?: string }> } }>) {
    total += v.description.length;
    for (const p of Object.values(v.inputSchema.properties ?? {})) total += (p.description ?? "").length;
  }
  if (total > 8000) fail(`base-budget (${total} chars > 8000)`);
  console.log(`  (base fixed cost: ${total} chars)`);
  pass("base-budget");
}
console.log("gate: green");
