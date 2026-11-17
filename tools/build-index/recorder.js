// Page init script for the scanner. Installed before the first navigation:
//   agent-browser --init-script tools/build-index/recorder.js --session S open <url>
// Hooks registerTool / unregisterTool / provideContext on both
// document.modelContext and navigator.modelContext and records every call
// with a timestamp, so the scanner can wait until the set stops changing.
(function () {
  if (window.__wmcHook) return;
  window.__wmcHook = true;
  window.__wmcLog = []; // {t, api, kind, name}
  window.__wmcLast = Date.now();
  function bump(api, kind, name) {
    try {
      window.__wmcLog.push({ t: Date.now(), api: api, kind: kind, name: String(name) });
      window.__wmcLast = Date.now();
    } catch (e) {}
  }
  function wrapOnOne(proto, obj, api, method, fn) {
    try {
      var orig = obj[method];
      if (typeof orig !== "function" || orig.__wmcWrapped) return false;
      var w = fn(orig);
      try { obj[method] = w; } catch (e) { return false; }
      w.__wmcWrapped = true;
      return true;
    } catch (e) { return false; }
  }
  function wrap(obj, api) {
    if (!obj) return;
    wrapOnOne(null, obj, api, "registerTool", function (orig) {
      return async function (tool) {
        bump(api, "register", tool && tool.name);
        return orig.call(this, tool);
      };
    });
    wrapOnOne(null, obj, api, "unregisterTool", function (orig) {
      return async function (name) {
        bump(api, "unregister", name);
        return orig.call(this, name);
      };
    });
    wrapOnOne(null, obj, api, "provideContext", function (orig) {
      return async function (ctx) {
        try {
          var tools = (ctx && ctx.tools) || [];
          for (var i = 0; i < tools.length; i++) bump(api + ".provideContext", "register", tools[i] && tools[i].name);
        } catch (e) {}
        return orig.call(this, ctx);
      };
    });
    // toolchange notifications also mark activity
    try {
      if (typeof obj.addEventListener === "function" && !obj.__wmcChangeHooked) {
        obj.__wmcChangeHooked = true;
        obj.addEventListener("toolchange", function () {
          try { window.__wmcLast = Date.now(); } catch (e) {}
        });
      }
    } catch (e) {}
  }
  function sweep() {
    try { if (document.modelContext) wrap(document.modelContext, "document.modelContext"); } catch (e) {}
    try { if (navigator.modelContext) wrap(navigator.modelContext, "navigator.modelContext"); } catch (e) {}
    // Older drafts used navigator.modelContext + provideContext(), which the
    // Chrome build does not offer. Answer it with a stub that logs and mirrors
    // into document.modelContext when it exists (Browser Use directory method).
    try {
      if (!navigator.modelContext && !navigator.__wmcStub) {
        var stub = {
          provideContext: async function (ctx) {
            try {
              var tools = (ctx && ctx.tools) || [];
              for (var i = 0; i < tools.length; i++) bump("navigator.modelContext", "register", tools[i] && tools[i].name);
            } catch (e) {}
            try {
              if (document.modelContext && document.modelContext.registerTool) {
                var ts = (ctx && ctx.tools) || [];
                for (var j = 0; j < ts.length; j++) { try { await document.modelContext.registerTool(ts[j]); } catch (e) {} }
              }
            } catch (e) {}
          },
          registerTool: async function (tool) {
            bump("navigator.modelContext", "register", tool && tool.name);
            try {
              if (document.modelContext && document.modelContext.registerTool) return await document.modelContext.registerTool(tool);
            } catch (e) {}
          },
          unregisterTool: async function (name) {
            bump("navigator.modelContext", "unregister", name);
            try {
              if (document.modelContext && document.modelContext.unregisterTool) return await document.modelContext.unregisterTool(name);
            } catch (e) {}
          },
          getTools: async function () {
            try {
              if (document.modelContext && document.modelContext.getTools) return await document.modelContext.getTools();
            } catch (e) {}
            return [];
          }
        };
        Object.defineProperty(navigator, "modelContext", { configurable: true, get: function () { return stub; } });
        navigator.__wmcStub = true;
      }
    } catch (e) {}
  }
  sweep();
  var iv = setInterval(sweep, 100);
  setTimeout(function () { clearInterval(iv); }, 120000);
})();
