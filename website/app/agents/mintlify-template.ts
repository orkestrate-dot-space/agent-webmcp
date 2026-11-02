// Single source of truth for every Mintlify-platform get_agent skill.
// Rows in sites.json carry only deltas; this template renders the full
// agent-to-agent contract. One observer core, five driver wrappers,
// per-site params interpolated — never duplicate this body per row.

export interface MintlifyDeltas {
  host: string; // e.g. "mintlify.com"
  docs: string; // docs base, e.g. "https://www.mintlify.com/docs"
  page: string; // deep-link page, e.g. "https://www.mintlify.com/docs/quickstart"
  searchTool: string; // from live tools/list, e.g. "search_mintlify"
  fsTool: string; // e.g. "query_docs_filesystem_mintlify"
  autoSubmit: boolean; // ?assistant=<msg> sends (claude.com) vs toggle-only (mintlify.com)
  description: string;
  verified: string;
}

// The observer core. Identical in every driver — only the wrapper
// changes. Watches the send control through arrow → STOP-BOX → arrow
// and resolves once the answer is settled: arrow back, text unchanged,
// quiet window elapsed. Handles two-phase hosts (STOP→arrow→STOP→arrow)
// by extending the quiet window when a second STOP appears.
export const OBSERVER_CORE = `(() => new Promise((resolve) => {
  const t0 = performance.now();
  const log = [{at: 0, state: "start"}];
  function root() {
    const host = Array.from(document.querySelectorAll("*")).find((e) => e.shadowRoot);
    if (host) { const d = host.shadowRoot.querySelector('[role="dialog"]'); if (d) return d; }
    return document.querySelector('[role="dialog"]');
  }
  function getState() {
    const d = root();
    if (!d) return {st: "no-panel", len: 0, tail: ""};
    const txt = d.innerText || "";
    let stop = false, send = null;
    d.querySelectorAll("button").forEach((b) => {
      const l = b.getAttribute("aria-label") || "";
      if (l.indexOf("Stop") !== -1) stop = true;
      if (b.className && b.className.indexOf("chat-assistant-send-button") !== -1) send = b;
    });
    if (stop) return {st: "STOP-BOX", len: txt.length, tail: txt.slice(-900)};
    if (send) return {st: "arrow|" + (send.disabled ? "disabled" : "enabled"), len: txt.length, tail: txt.slice(-900)};
    return {st: "no-composer", len: txt.length, tail: txt.slice(-900)};
  }
  let stops = 0, quietSince = 0, lastLen = -1;
  const iv = setInterval(() => {
    const cur = getState();
    const last = log[log.length - 1];
    if (!last || last.state !== cur.st) { log.push({at: Math.round(performance.now() - t0), state: cur.st, len: cur.len}); if (cur.st === "STOP-BOX") stops++; }
    if (cur.len !== lastLen) { lastLen = cur.len; quietSince = 0; }
    else if (cur.st.indexOf("arrow|") === 0 && stops >= 1) {
      if (!quietSince) quietSince = performance.now();
      const need = stops >= 2 ? 3000 : 1000;
      if (performance.now() - quietSince >= need) {
        clearInterval(iv);
        resolve({timeline: log, totalMs: Math.round(performance.now() - t0), tail: cur.tail, len: cur.len, stops});
      }
    } else if (cur.st !== "STOP-BOX") { quietSince = 0; }
  }, 100);
  setTimeout(() => { clearInterval(iv); const f = getState(); resolve({timeline: log, totalMs: Math.round(performance.now() - t0), note: "TIMEOUT-45s", tail: f.tail, len: f.len, stops}); }, 45000);
}))()`;

const SEND_CORE = `(question) => {
  const d = (() => {
    const host = Array.from(document.querySelectorAll("*")).find((e) => e.shadowRoot);
    if (host) { const x = host.shadowRoot.querySelector('[role="dialog"]'); if (x) return x; }
    return document.querySelector('[role="dialog"]');
  })();
  const box = d.querySelector("textarea");
  const proto = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value");
  proto.set.call(box, question);
  box.dispatchEvent(new Event("input", {bubbles: true}));
  const send = d.querySelector("button.chat-assistant-send-button");
  if (send && !send.disabled) send.click();
}`;

export function mintlifySkill(d: MintlifyDeltas): string {
  const deepLink = d.autoSubmit
    ? `${d.page}?assistant={URLEncoded-first-question} — opens AND auto-submits; if no composer appears, click #assistant-entry (aria-label "Toggle assistant panel") and send manually`
    : `${d.page}?assistant — opens the panel; click #assistant-entry (aria-label "Toggle assistant panel"), then send manually`;
  return [
    `ASK ${d.host} docs — Mintlify site agent. Reasoning chat with citations, multi-turn threads, refusals when out of scope. ${d.description}`,
    ``,
    `TIER 1 — CHAT (the agent; headed browser, no key, ~10–20s/turn):`,
    `1. Open ${deepLink}.`,
    `2. Send with the proto-setter + input event (direct .value breaks React state), then click Send.`,
    `3. Watch the send control: arrow → STOP-BOX → arrow.`,
    `4. Settled = arrow back, text unchanged, quiet window elapsed (1s, 3s on two-phase hosts). Read the panel text for the answer with inline sources.`,
    `5. Follow-ups stay in the same tab. Use a pronoun ("How is that different?") to verify the thread carried.`,
    `Limits: headed non-bot browser only (captchas gate headless); same-tab threads; never paste secrets (panel redacts).`,
    ``,
    `TIER 2 — MCP RETRIEVAL (headless, no key, no reasoning): POST ${d.docs.replace(/\/$/, "")}/mcp tools/call ${d.searchTool} {query} for passages, ${d.fsTool} {command: "head -200 /<path>.mdx"} for full pages; index at ${d.docs.replace(/\/$/, "")}/llms.txt.`,
    ``,
    `TIER 3 — KEYED NATIVE (owner only): POST https://api.mintlify.com/discovery/v2/assistant/{domain}/message, Bearer mint_dsc_…, body {fp, messages[]}; follow-ups resend threadId AND threadKey both (threadId alone → 404). 25 credits/answer; search/page cost 0. Needs the site owner's key — do not attempt without one.`,
    ``,
    `Verified: ${d.verified}. Chat on this host: ${d.autoSubmit ? "auto-submits via deep link" : "toggle + manual send"}.`,
  ].join("\n");
}

export function mintlifyRegisterDef(d: MintlifyDeltas): { name: string; description: string } {
  const short = d.host.split(".")[0];
  return {
    name: `ask_${short}`,
    description: `Ask ${d.host} docs; reasoning answers with citations. Tier 1 headed chat, Tier 2 headless MCP fallback.`,
  };
}

export const DRIVERS = ["agent-webmcp", "puppeteer", "stagehand", "raw-cdp", "playwright-mcp"] as const;
export type Driver = (typeof DRIVERS)[number];

// Full runnable recipe per driver. Cores interpolate from the single
// source above — every tab shows complete code, no opaque names.
// (Parked: the /agents/{domain} Code section is removed for now; the
// get_agent payload still ships observerCore + sendCore separately.)
export function driverSnippet(d: MintlifyDeltas, driver: Driver): string {
  const q = "How do I get started?";
  const deepLink = d.autoSubmit
    ? `${d.page}?assistant=${encodeURIComponent(q)}`
    : `${d.page}?assistant`;
  const toggle = d.autoSubmit
    ? `// deep link auto-submits; if no composer appears, click #assistant-entry ("Toggle assistant panel")`
    : `// panel opens without a question: click #assistant-entry ("Toggle assistant panel"), then send (next line)`;
  const sendCall = `(${SEND_CORE})(${JSON.stringify(q)})`;
  switch (driver) {
    case "agent-webmcp":
      return [
        `// ${d.host} docs agent — reasoning chat. Headed browser, no key, ~10-20s/turn.`,
        `const agent = await tools.get_agent({ domain: "${d.host}" }); // skill + sendCore + observerCore`,
        `const tab = await tools.browser.tabs.open({ url: "${deepLink}" });`,
        `await tools.browser.wait({ tabID: tab.id, condition: "load" });`,
        toggle,
        `await tools.browser.evaluate({ tabID: tab.id, script: \`(\${agent.sendCore})(${JSON.stringify(q)})\` });`,
        `const r = await tools.browser.evaluate({ tabID: tab.id, script: agent.observerCore });`,
        `// r.value.tail = answer with sources. Same tab for follow-ups.`,
        `// cores in full: agent.sendCore, agent.observerCore (puppeteer tab shows them inline).`,
      ].join("\n");
    case "puppeteer":
      return [
        `import puppeteer from "puppeteer";`,
        `// headed only: the chat is captcha-gated, headless never passes.`,
        `const browser = await puppeteer.launch({ headless: false });`,
        `const page = await browser.newPage();`,
        `await page.goto("${deepLink}", { waitUntil: "domcontentloaded" });`,
        toggle,
        `await page.evaluate(() => { ${sendCall}; });`,
        `const r = await page.evaluate(() => { return (${OBSERVER_CORE}); });`,
        `// r.tail = answer with sources. Same page for follow-ups.`,
      ].join("\n");
    case "stagehand":
      return [
        `import { Stagehand } from "@browserbasehq/stagehand";`,
        `// headed only: the chat is captcha-gated, headless never passes.`,
        `const stagehand = new Stagehand({ headless: false });`,
        `await stagehand.init();`,
        `await stagehand.goto("${deepLink}");`,
        toggle,
        `await stagehand.evaluate(() => { ${sendCall}; });`,
        `const r = await stagehand.evaluate(() => { return (${OBSERVER_CORE}); });`,
        `// r.tail = answer with sources. Same tab for follow-ups.`,
      ].join("\n");
    case "raw-cdp":
      return [
        `// any CDP client (agent-browser, chrome-remote-interface, …). Target: a headed, non-bot tab.`,
        `// 1. Page.navigate to the deep link, wait for load.`,
        `//    url: ${deepLink}`,
        toggle,
        `// 2. send (Runtime.evaluate, plain call):`,
        `//    expression: ${sendCall}`,
        `// 3. observe (Runtime.evaluate with awaitPromise:true — the core returns a Promise):`,
        `//    expression: ${OBSERVER_CORE}`,
        `// result.result.value.tail = answer with sources. Reuse the same targetId for follow-ups.`,
      ].join("\n");
    case "playwright-mcp":
      return [
        `// Playwright MCP harness verbs. Attach to a headed Chromium tab first.`,
        `browser_navigate({ url: "${deepLink}" })`,
        toggle,
        `browser_evaluate({ function: "() => { ${sendCall}; return 'sent'; }" })`,
        `browser_evaluate({ function: "() => { return (${OBSERVER_CORE}); }" })`,
        `// return value .tail = answer with sources. Same tab for follow-ups.`,
      ].join("\n");
  }
}

// One-line wrapper notes per driver. The OBSERVER_CORE and SEND_CORE
// above drop in unchanged — these lines are the only per-driver code.
export function driverNote(driver: Driver): string {
  switch (driver) {
    case "agent-webmcp":
      return `open tab, wait load, one evaluate(SEND_CORE with your question) then one evaluate(OBSERVER_CORE). One execute call per turn.`;
    case "puppeteer":
      return `await page.goto(url); await page.evaluate(SEND_CORE); await page.evaluate(OBSERVER_CORE); // same strings, page.evaluate wrapper`;
    case "stagehand":
      return `await stagehand.goto(url); await stagehand.evaluate(SEND_CORE); await stagehand.evaluate(OBSERVER_CORE);`;
    case "raw-cdp":
      return `Page.navigate, then Runtime.evaluate with SEND_CORE, then Runtime.evaluate with OBSERVER_CORE. Poll nothing outside the page.`;
    case "playwright-mcp":
      return `browser_navigate(url), browser_evaluate(SEND_CORE), browser_evaluate(OBSERVER_CORE).`;
  }
}

export { SEND_CORE };
