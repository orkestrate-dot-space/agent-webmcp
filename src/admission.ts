// Admission: files become live verbs. Two entries, one gate:
// - reapply (on open): stamped + unchanged bytes re-inject silently;
//   changed bytes re-prove; bad shapes and unknown names skip with reasons.
// - admit (on first execute of a disk name): validate → native-collision
//   check → inject → prove → stamp. Never fails an open; never injects a
//   bad file; never overwrites a native verb.
import { evaluateJson, invokePageTool, listPageTools, type CdpConnection, type PageTool } from "./transport.ts";
import { bodyHashOf, listOriginTools, loadTool, stampTool, type LoadedTool } from "./registry.ts";
import { fail } from "./result.ts";
import { parseRegisterReply, registerSnippet, unregisterSnippet } from "./snippet.ts";

export interface AdmitSkipped { readonly name: string; readonly reason: string }

async function liveNames(conn: CdpConnection, sessionId: string, timeoutMs: number): Promise<Set<string>> {
  const live = await listPageTools(conn, timeoutMs, sessionId).catch(() => [] as Array<PageTool>);
  return new Set(live.map((t) => t.name));
}

function defJson(spec: LoadedTool["spec"]): string {
  return JSON.stringify({
    name: spec.name, description: spec.description, inputSchema: spec.inputSchema,
    ...(spec.annotations !== undefined ? { annotations: spec.annotations } : {}),
  });
}

async function inject(conn: CdpConnection, sessionId: string, loaded: LoadedTool, timeoutMs: number): Promise<string | null> {
  let value: unknown;
  try {
    value = await evaluateJson(conn, registerSnippet(loaded.spec.name, defJson(loaded.spec), loaded.body), timeoutMs, sessionId);
  } catch (e) {
    return e instanceof Error ? e.message.slice(0, 200) : "injection failed";
  }
  return parseRegisterReply(value, loaded.spec.name);
}

// Proof: run the fixture once. Classifies the outcome; thrown text is the
// reason (native invoke may drop it — the classifier surfaces whatever the
// page did return before falling back, never an empty diagnostic).
export async function proveTool(
  conn: CdpConnection, sessionId: string, frameId: string, name: string, args: Record<string, unknown>, timeoutMs: number
): Promise<{ ok: true } | { ok: false; why: string }> {
  const nonBlank = (v: unknown, fallback: string): string => {
    const text = String(v ?? "").trim();
    return text === "" || text === "undefined" ? fallback : text.slice(0, 200);
  };
  let out: { status: string; output?: unknown; errorText?: string };
  try {
    out = await invokePageTool(conn, sessionId, { frameId, toolName: name, args }, timeoutMs);
  } catch (e) {
    return { ok: false, why: nonBlank(e instanceof Error ? e.message : e, "transport failure") };
  }
  if (out.status === "Error") {
    if (typeof out.errorText === "string" && out.errorText.trim() !== "") return { ok: false, why: out.errorText.slice(0, 200) };
    if (out.output !== undefined && out.output !== null) {
      const dumped = (typeof out.output === "string" ? out.output : JSON.stringify(out.output)).trim();
      if (dumped !== "") return { ok: false, why: dumped.slice(0, 200) };
    }
    return { ok: false, why: "page reported failure" };
  }
  return { ok: true };
}

export async function reapplyOrigin(
  conn: CdpConnection, sessionId: string, origin: string, timeoutMs: number
): Promise<{ reapplied: Array<string>; skipped: Array<AdmitSkipped> }> {
  const reapplied: Array<string> = [];
  const skipped: Array<AdmitSkipped> = [];
  const names = listOriginTools(origin);
  if (names.length === 0) return { reapplied, skipped };
  const live = await liveNames(conn, sessionId, timeoutMs);
  for (const name of names) {
    if (live.has(name)) { skipped.push({ name, reason: "already live (native or shared session) — never overwritten." }); continue; }
    let loaded: LoadedTool;
    try { loaded = loadTool(origin, name); }
    catch (e) { skipped.push({ name, reason: `${e instanceof Error ? e.message : String(e)} — files kept.` }); continue; }
    const hash = bodyHashOf(loaded.body);
    const stamped = loaded.stamp !== null && loaded.stamp.bodyHash === hash;
    const consequential = loaded.spec.consequential === true;
    // Unproven consequential verbs never execute on open: no confirmed
    // call exists yet, and open carries no confirm. Files kept; the
    // author hand-proves with confirm:true, then re-open re-applies.
    if (consequential && !stamped) {
      skipped.push({ name, reason: "unstamped consequential (no proving run yet) — call it once with confirm:true, files kept." });
      continue;
    }
    const refusal = await inject(conn, sessionId, loaded, timeoutMs);
    if (refusal !== null) { skipped.push({ name, reason: `${refusal} — files kept.` }); continue; }
    const present = await liveNames(conn, sessionId, timeoutMs);
    if (!present.has(name)) { skipped.push({ name, reason: "registered but absent from catalog — files kept." }); continue; }
    if (stamped) { reapplied.push(name); continue; } // fast path: these bytes already proved
    if (!consequential && loaded.spec.fixtureInput !== undefined) {
      const frames = await listPageTools(conn, timeoutMs, sessionId).catch(() => [] as Array<PageTool>);
      const found = frames.find((t) => t.name === name);
      const verdict = await proveTool(conn, sessionId, found?.frameId ?? "", name, loaded.spec.fixtureInput as Record<string, unknown>, timeoutMs);
      if (!verdict.ok) {
        // Proof failed: roll back the inject. The page must not hold
        // what no proof covers (and a live-but-unproven verb would
        // poison dispatch into a false collision).
        await evaluateJson(conn, unregisterSnippet(name), timeoutMs, sessionId).catch(() => undefined);
        skipped.push({ name, reason: `proof failed (${verdict.why}) — files kept.` });
        continue;
      }
      stampTool(origin, name, hash);
      reapplied.push(name);
      continue;
    }
    // No proving path at open: roll back the inject (the page must not
    // hold what no proof covers) and surface as unverified in list.
    await evaluateJson(conn, unregisterSnippet(name), timeoutMs, sessionId).catch(() => undefined);
    skipped.push({ name, reason: "no proving path at open (no fixture, no stamp) — add fixtureInput or hand-prove, files kept." });
  }
  return { reapplied, skipped };
}

// Lazy admission for execute: the named verb is on disk but not live.
// (Callers only admit names absent from the live catalog; same-name live
// verbs are handled at dispatch — stamped ours calls through, unstamped
// disk files colliding with a live verb fail loud there.)
export async function admitOnCall(
  conn: CdpConnection, sessionId: string, origin: string, name: string,
  opts: { confirm: boolean; timeoutMs: number }
): Promise<{ frameId: string; consequential: boolean; stamp: () => void }> {
  let loaded: LoadedTool;
  try { loaded = loadTool(origin, name); }
  catch (e) { throw fail("NO_SUCH_TOOL", e instanceof Error ? e.message : String(e), "write spec.json plus body.ts first, or call a listed verb."); }
  const hash = bodyHashOf(loaded.body);
  const stamped = loaded.stamp !== null && loaded.stamp.bodyHash === hash;
  const consequential = loaded.spec.consequential === true;
  // Refusals precede injection: nothing lands in the page unless a proving
  // path exists.
  if (!stamped && consequential && !opts.confirm) {
    throw fail("NEEDS_CONFIRM", `'${name}' is consequential with no proving run for these bytes`, "resend this call with confirm:true — the confirmed call itself is the proof.");
  }
  if (!stamped && !consequential && loaded.spec.fixtureInput === undefined) {
    throw fail("NO_PROOF_PATH", `'${name}' has no fixtureInput and is not consequential`, "add fixtureInput to the spec (a provable example), or mark it consequential and call with confirm:true.");
  }
  const refusal = await inject(conn, sessionId, loaded, opts.timeoutMs);
  if (refusal !== null) throw fail("PAGE_REFUSED", refusal, "fix the file or the page; nothing was injected.");
  const after = await listPageTools(conn, opts.timeoutMs, sessionId).catch(() => [] as Array<PageTool>);
  const frameId = after.find((t) => t.name === name)?.frameId;
  if (frameId === undefined) throw fail("PAGE_REFUSED", `'${name}' registered but absent from catalog`, "the page dropped it — re-list, then retry once.");
  if (!stamped) {
    if (consequential) {
      // The confirmed call below is the proof — stamp only after it
      // succeeds. The caller invokes stamp() on success.
      return { frameId, consequential, stamp: () => stampTool(origin, name, hash) };
    }
    const verdict = await proveTool(conn, sessionId, frameId, name, loaded.spec.fixtureInput as Record<string, unknown>, opts.timeoutMs);
    if (!verdict.ok) {
      await evaluateJson(conn, unregisterSnippet(name), opts.timeoutMs, sessionId).catch(() => undefined);
      throw fail("PROOF_FAILED", `fixture failed (${verdict.why})`, "fix the body against the reason; files kept, nothing live.");
    }
    stampTool(origin, name, hash);
  }
  return { frameId, consequential, stamp: () => {} };
}
