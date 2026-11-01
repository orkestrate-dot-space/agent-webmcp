// Authored site agents: contract registry + per-platform drivers.
// A /agents row is data (domain, tool name, platform, endpoint); the
// driver below is engine code. No arbitrary bodies are ever admitted:
// a row can only pick a platform the engine already knows how to
// drive. Mintlify rows share one driver (deep-link + panel read);
// each new platform gets its own run* driver + a row per verified site.
import { evaluateJson, type CdpConnection } from "./transport.ts";
import { fail } from "./result.ts";

export interface AgentContract {
  readonly domain: string;
  readonly tool: string;
  readonly description: string;
  readonly platform: "mintlify";
  readonly transport: "deep-link";
  readonly endpoint: string;
}

const CONTRACTS: Array<AgentContract> = [
  {
    domain: "mintlify.com",
    tool: "ask_mintlify",
    description: "Ask Mintlify's docs assistant anything; returns its worked answer with citations.",
    platform: "mintlify",
    transport: "deep-link",
    endpoint: "https://www.mintlify.com/docs/<page>?assistant=<question>",
  },
  {
    domain: "claude.com",
    tool: "ask_claude",
    description: "Ask Claude's docs assistant anything; grounded answers with page citations, keeps the thread.",
    platform: "mintlify",
    transport: "deep-link",
    endpoint: "https://claude.com/docs/connectors/getting-started?assistant=<question>",
  },
];

export function matchAgent(origin: string): AgentContract | undefined {
  let host: string;
  try {
    host = new URL(origin).hostname.toLowerCase();
  } catch {
    return undefined;
  }
  return CONTRACTS.find((c) => host === c.domain || host.endsWith(`.${c.domain}`));
}

export function agentInputSchema(): Record<string, unknown> {
  return {
    type: "object",
    properties: { question: { type: "string", description: "The question for the site's assistant." } },
    required: ["question"],
    additionalProperties: false,
  };
}

// Mintlify platform driver: mint a hidden tab on the session's own
// browser, open the assistant with the question in the URL, wait for
// the settled answer text, close the tab. The user's borrowed/minted
// tab is never navigated. Mintlify chat is bot-gated, so this needs a
// non-bot browser; refusing browsers fail loud with the fix.
export async function runMintlifyAssistant(
  conn: CdpConnection,
  endpoint: string,
  question: string,
  timeoutMs: number,
  toolName: string
): Promise<string> {
  const url = endpoint.replace("<page>", "quickstart").replace("<question>", encodeURIComponent(question));
  let targetId: string;
  try {
    const created = (await conn.send("Target.createTarget", { url })) as { targetId?: unknown };
    if (typeof created.targetId !== "string" || created.targetId === "") throw 0;
    targetId = created.targetId;
  } catch {
    throw fail(
      "mint",
      `browser refuses new tabs — ${toolName} needs a minting browser for its assistant tab.`,
      "open via native Chromium (:9222), not a borrow-only window."
    );
  }
  try {
    const attached = (await conn.send("Target.attachToTarget", { targetId, flatten: true })) as { sessionId?: unknown };
    if (typeof attached.sessionId !== "string" || attached.sessionId === "") {
      throw fail("page", "assistant tab would not attach.", "retry once, then open again.");
    }
    const sessionId = attached.sessionId;
    // The minted tab commits about:blank before the real page, so a bare
    // readyState check passes on the wrong document. Wait for the
    // assistant's own panel instead — it only exists once the docs app
    // has mounted and (with ?assistant=) auto-submitted.
    const born = Date.now();
    for (;;) {
      try {
        const present = await evaluateJson(
          conn,
          "(function(){return !!document.querySelector('INPUT.chat-assistant-input,[role=dialog],[role=complementary]');})()",
          10000,
          sessionId
        );
        if (present === true) break;
      } catch {
        /* mid-navigation or pre-mount — keep waiting */
      }
      if (Date.now() - born > 90000) {
        throw fail("page", "assistant panel never mounted.", "retry once — the site may be slow or its markup changed (re-verify the row).");
      }
      await new Promise((r) => setTimeout(r, 1000));
    }
    const deadline = Math.max(10000, Math.min(timeoutMs - 5000, 240000));
    const marker = JSON.stringify(question.slice(0, 20));
    const expr =
      `(async (deadlineMs, marker) => { const t0 = Date.now(); let prev = ''; ` +
      `while (Date.now() - t0 < deadlineMs) { let best = ''; ` +
      `for (const el of document.querySelectorAll('[role=dialog],[role=complementary],aside')) { ` +
      `const tx = (el.innerText || '').trim(); ` +
      `if (marker !== '' && tx.indexOf(marker) === -1) continue; ` +
      `if (tx.length > best.length) best = tx; } ` +
      `if (best.length > 200 && best === prev) return 'DONE >>> ' + best.slice(0, 4000); ` +
      `if (best) prev = best; ` +
      `await new Promise(function (r) { setTimeout(r, 1000); }); } ` +
      `return 'TIMEOUT >>> ' + prev.slice(0, 500); })(${deadline}, ${marker})`;
    const raw = await evaluateJson(conn, expr, timeoutMs, sessionId);
    if (typeof raw !== "string" || !raw.startsWith("DONE >>> ")) {
      throw fail(
        "page",
        `assistant produced no settled answer (${typeof raw === "string" ? raw.slice(0, 120) : "no text"}).`,
        "the site may have changed its chat markup — re-verify the row, or retry once."
      );
    }
    return raw.slice("DONE >>> ".length);
  } finally {
    try {
      await conn.send("Target.closeTarget", { targetId });
    } catch {
      /* tab already gone — success either way */
    }
  }
}
