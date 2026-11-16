(function () {
  if (window.__awmExampleLoaded) return;
  window.__awmExampleLoaded = true;
  var mc = document.modelContext;
  if (!mc || typeof mc.registerTool !== "function") return;

  mc.registerTool({
    name: "say_hello",
    description: "Say hello to someone by name.",
    inputSchema: {
      type: "object",
      properties: { name: { type: "string", description: "Who to greet" } },
      required: ["name"]
    },
    annotations: { readOnlyHint: true },
    execute: async function (args) {
      return { content: [{ type: "text", text: "hello " + args.name }] };
    }
  });

  mc.registerTool({
    name: "leave_note",
    description: "Leave a short note on the page. Changes the page, so it needs approval.",
    inputSchema: {
      type: "object",
      properties: { text: { type: "string", description: "The note" } },
      required: ["text"]
    },
    annotations: { readOnlyHint: false },
    execute: async function (args) {
      var el = document.createElement("p");
      el.setAttribute("data-awm-note", "");
      el.textContent = args.text;
      document.body.appendChild(el);
      return { content: [{ type: "text", text: "noted: " + args.text }] };
    }
  });
})();
