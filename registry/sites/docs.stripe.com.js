// Custom tools for docs.stripe.com (agent-webmcp Phase 3: ask_ai hybrid).
// The site ships no native WebMCP tools. This drives the built-in
// Stripe Assistant drawer: open -> ask -> wait for stream settle -> answer.

document.modelContext.registerTool({
  name: "ask_ai",
  description: "Ask the Stripe docs AI assistant a question. Returns the markdown answer. Use for how-to, API behavior, and concept questions about Stripe.",
  inputSchema: {
    type: "object",
    required: ["question"],
    properties: {
      question: { type: "string", description: "Technical question about Stripe." }
    }
  },
  annotations: { readOnlyHint: true },
  execute: async ({ question }) => {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const openDrawer = async () => {
      if (document.getElementById("StripeAssistant")) return;
      const btn = document.querySelector('[data-testid="StripeAssistant--Header-AskAI"]');
      if (!btn) throw new Error("Stripe Assistant button not found on this page");
      btn.click();
      for (let i = 0; i < 40; i++) {
        if (document.getElementById("StripeAssistant")) return;
        await sleep(250);
      }
      throw new Error("Stripe Assistant drawer did not open");
    };
    const settle = async (getText, { quietMs = 1500, timeoutMs = 45000, minChars = 40 } = {}) => {
      let last = "";
      let stableSince = Date.now();
      const deadline = Date.now() + timeoutMs;
      while (Date.now() < deadline) {
        const text = String(getText() || "").trim();
        if (text !== last) {
          last = text;
          stableSince = Date.now();
        } else if (text.length >= minChars && Date.now() - stableSince >= quietMs) {
          return text;
        }
        await sleep(300);
      }
      if (last.length >= minChars) return last;
      throw new Error("Assistant answer did not settle in time");
    };
    await openDrawer();
    const root = document.getElementById("StripeAssistant");
    const ta = root.querySelector("textarea");
    if (!ta) throw new Error("Assistant input not found");
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set;
    setter.call(ta, String(question));
    ta.dispatchEvent(new Event("input", { bubbles: true }));
    ta.dispatchEvent(new Event("change", { bubbles: true }));
    await sleep(400);
    for (const type of ["keydown", "keypress"]) {
      ta.dispatchEvent(new KeyboardEvent(type, { key: "Enter", code: "Enter", keyCode: 13, bubbles: true }));
    }
    const answer = await settle(() => {
      const el = document.getElementById("StripeAssistant");
      return el ? el.innerText : "";
    });
    const lines = answer.split("\n").map((l) => l.trim()).filter(Boolean);
    return { answer: lines.join("\n"), lines: lines.length };
  }
});
