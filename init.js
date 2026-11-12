/* agent-webmcp init.js v0 — passive WebMCP registry only.
 * No browser driving, no network, no DOM writes.
 * - Defines document.modelContext + navigator.modelContext if absent.
 * - Wraps native executeTool to accept object AND string (routes around
 *   anomalyco/browse#121 on stable: string in, parse, run).
 * - window.webmcp facade for execute code-mode: list/call/define.
 * Self-contained IIFE: safe for addInitScript, --init-script, eval. */
(function () {
  "use strict";
  var root = typeof window !== "undefined" ? window : globalThis;
  var doc = typeof document !== "undefined" ? document : null;
  if (!doc) return;

  function normalizeInput(input) {
    if (typeof input === "string") {
      try { return JSON.parse(input); } catch (e) { throw new Error("Failed to parse input arguments"); }
    }
    return input === undefined || input === null ? {} : input;
  }

  // Merge locally cached tool summaries with an upstream native list.
  // First list wins on name collisions; upstream entries never shadow local.
  function mergeToolLists(cached, upstream) {
    var arr = Array.isArray(upstream) ? upstream : [];
    var have = {};
    var i, t, n, c;
    for (i = 0; i < cached.length; i++) have[cached[i].name] = true;
    var merged = cached.slice();
    for (i = 0; i < arr.length; i++) {
      t = arr[i];
      n = t && t.name;
      if (typeof n !== "string" || have[n]) continue;
      have[n] = true;
      c = { name: n };
      if (typeof t.description === "string") c.description = t.description;
      merged.push(c);
    }
    return merged;
  }

  // window.webmcp.<toolName> shorthands, bound at register time so the
  // snippets `get` emits actually resolve. Never shadows list/call/define.
  function bindShorthand(name) {
    try {
      var wm = root.webmcp;
      if (!wm || typeof name !== "string") return;
      if (name === "list" || name === "call" || name === "define") return;
      if (Object.prototype.hasOwnProperty.call(wm, name)) return;
      Object.defineProperty(wm, name, {
        value: function (input, opts) { return wm.call(name, input === undefined ? {} : input, opts); },
        configurable: true,
        enumerable: true,
      });
    } catch (e) {}
  }
  function unbindShorthand(name) {
    try {
      var wm = root.webmcp;
      if (!wm || typeof name !== "string") return;
      if (name === "list" || name === "call" || name === "define") return;
      try { delete wm[name]; } catch (e) {}
    } catch (e) {}
  }

  function makeRegistry(native) {
    var tools = new Map();
    // Custom tools registered through this header carry their own execute().
    // Native executeTool takes a browser-blessed handle (with .origin), so a
    // custom def must run locally, never via native.executeTool.
    var custom = {};
    // Local descriptions win getTools() collisions against same-named
    // upstream entries (the re-found handle may predate this registration).
    var localDesc = {};
    // Native handles: Chrome 157+ executeTool takes the registered tool
    // object, not its name. Cache name -> handle at register time, since
    // native getTools() is opaque and pre-existing tools are unresolvable.
    var handles = new Map();
    var listeners = new Set();
    function emit() {
      var evt;
      try { evt = new Event("toolchange"); } catch (e) { evt = { type: "toolchange" }; }
      listeners.forEach(function (fn) { try { fn(evt); } catch (e) {} });
      if (doc.dispatchEvent) { try { doc.dispatchEvent(new Event("webmcp:toolchange")); } catch (e) {} }
    }
    var registry = {
      registerTool: function (tool, opts) {
        if (!tool || typeof tool.name !== "string") throw new Error("registerTool needs {name}");
        if (typeof tool.execute === "function") { try { custom[tool.name] = tool; } catch (e) {} }
        if (typeof tool.description === "string") { try { localDesc[tool.name] = tool.description; } catch (e) {} }
        if (native) {
          var r = native.registerTool.call(native, tool, opts);
          try {
            var p = Promise.resolve(r).then(function () { return native.getTools.call(native); }).then(function (list) {
              var arr = Array.isArray(list) ? list : [];
              for (var i = 0; i < arr.length; i++) {
                try { if (arr[i] && arr[i].name === tool.name) { handles.set(tool.name, arr[i]); return arr[i]; } } catch (e) {}
              }
              handles.set(tool.name, tool);
              return tool;
            }, function () { return tool; });
            handles.set(tool.name, p);
          } catch (e) {}
          bindShorthand(tool.name);
          emit();
          if (opts && opts.signal) {
            if (opts.signal.aborted) { try { registry.unregisterTool(tool.name); } catch (e) {} }
            else opts.signal.addEventListener("abort", function () { try { registry.unregisterTool(tool.name); } catch (e) {} }, { once: true });
          }
          return r;
        }
        tools.set(tool.name, tool);
        bindShorthand(tool.name);
        emit();
        if (opts && opts.signal) {
          if (opts.signal.aborted) { tools.delete(tool.name); unbindShorthand(tool.name); }
          else opts.signal.addEventListener("abort", function () { tools.delete(tool.name); unbindShorthand(tool.name); emit(); }, { once: true });
        }
      },
      unregisterTool: function (name) {
        try { delete custom[name]; } catch (e) {}
        try { delete localDesc[name]; } catch (e) {}
        if (native && native.unregisterTool) { var r = native.unregisterTool.call(native, name); unbindShorthand(name); emit(); return r; }
        var ok = tools.delete(name);
        if (ok) { unbindShorthand(name); emit(); }
        return ok;
      },
      getTools: function () {
        if (native) {
          // Merge upstream natives with locally cached registrations (local
          // wins on name collisions). Never leak pending Promise handles.
          var cached = [];
          handles.forEach(function (t, n) {
            var c0 = { name: n };
            // Local description first: synchronous at register time, so the
            // listing is correct even while the handle promise is pending.
            if (Object.prototype.hasOwnProperty.call(localDesc, n)) c0.description = localDesc[n];
            else if (t && typeof t.then !== "function" && typeof t.description === "string") c0.description = t.description;
            cached.push(c0);
          });
          var up;
          try { up = native.getTools.call(native); } catch (e) { up = null; }
          if (up === null) return Promise.resolve(cached);
          return Promise.resolve(up).then(function (list) {
            return mergeToolLists(cached, list);
          }, function () { return cached; });
        }
        return Array.from(tools.values()).map(function (t) {
          var c = {};
          for (var k in t) if (k !== "execute" && Object.prototype.hasOwnProperty.call(t, k)) c[k] = t[k];
          return c;
        });
      },
      executeTool: async function (name, input) {
        var args = normalizeInput(input);
        if (native) {
          // Resolve the registered handle first: native takes the tool
          // object, not its name. Uncached names fall through to a
          // name call for older implementations.
          var handle = null;
          try { handle = handles.get(name) || null; } catch (e) {}
          if (handle !== null && typeof handle.then === "function") {
            try { handle = await handle; } catch (e) { handle = null; }
          }
          if (handle === null) {
            try {
              var relist = await native.getTools.call(native);
              var arr = Array.isArray(relist) ? relist : [];
              for (var i = 0; i < arr.length; i++) {
                try { if (arr[i] && arr[i].name === name) { handle = arr[i]; handles.set(name, handle); break; } } catch (e) {}
              }
            } catch (e) {}
          }
          var target = handle !== null ? handle : name;
          var out;
          try { out = await native.executeTool.call(native, target, args); }
          catch (e) {
            var msg = String((e && e.message) || e);
            // Custom defs registered through this header are plain objects
            // without a browser-blessed .origin: native rejects them. Run
            // the captured local execute() instead of failing the call.
            if (/origin|RegisteredTool/i.test(msg)) {
              var fallback = null;
              try { fallback = custom[name] || null; } catch (e2) {}
              if (fallback && typeof fallback.execute === "function") return await fallback.execute(args);
            }
            if (/not of type ['"]?object['"]?|Failed to parse input arguments/i.test(msg) && typeof input !== "string") {
              out = await native.executeTool.call(native, target, JSON.stringify(args));
            } else throw e;
          }
          if (typeof out === "string") {
            try { return JSON.parse(out); } catch (e) { return out; }
          }
          return out;
        }
        var tool = tools.get(name);
        if (!tool) throw new Error("unknown tool: " + name);
        if (typeof tool.execute !== "function") throw new Error("tool has no execute: " + name);
        return await tool.execute(args);
      },
      addEventListener: function (type, fn) { if (type === "toolchange") listeners.add(fn); },
      removeEventListener: function (type, fn) { if (type === "toolchange") listeners.delete(fn); },
    };
    return registry;
  }

  var nativeCtx = doc.modelContext || (typeof navigator !== "undefined" && navigator.modelContext) || null;
  var hasNativeExecute = !!(nativeCtx && typeof nativeCtx.executeTool === "function");
  var registry = makeRegistry(hasNativeExecute ? nativeCtx : null);

  try {
    if (!doc.modelContext) Object.defineProperty(doc, "modelContext", { value: registry, configurable: true, writable: true });
    else if (hasNativeExecute && !doc.modelContext.__awLenient) {
      // Chrome 157 native: registerTool/getTools are async, and
      // executeTool takes the tool handle (with .origin filled in), not
      // its name. Cache name -> handle at register time by re-listing.
      var seen = {};
      var customTools = {};
      var localDesc157 = {};
      var origReg = doc.modelContext.registerTool.bind(doc.modelContext);
      var origList = doc.modelContext.getTools.bind(doc.modelContext);
      var origExec = doc.modelContext.executeTool.bind(doc.modelContext);
      var findHandle = async function (name) {
        try {
          var list = await origList();
          var arr = Array.isArray(list) ? list : [];
          for (var i = 0; i < arr.length; i++) {
            try { if (arr[i] && arr[i].name === name) return arr[i]; } catch (e) {}
          }
        } catch (e) {}
        return null;
      };
      doc.modelContext.registerTool = function (tool, opts) {
        var r = origReg(tool, opts);
        try {
          if (tool && typeof tool.name === "string") {
            if (typeof tool.execute === "function") { try { customTools[tool.name] = tool; } catch (e) {} }
            if (typeof tool.description === "string") { try { localDesc157[tool.name] = tool.description; } catch (e) {} }
            var p = Promise.resolve(r).then(function () { return findHandle(tool.name); }).then(function (h) {
              seen[tool.name] = h || tool;
              return seen[tool.name];
            }, function () { return tool; });
            seen[tool.name] = p;
            bindShorthand(tool.name);
            if (opts && opts.signal) {
              var onAbort = function () { try { delete seen[tool.name]; } catch (e) {} try { delete customTools[tool.name]; } catch (e) {} try { delete localDesc157[tool.name]; } catch (e) {} unbindShorthand(tool.name); };
              if (opts.signal.aborted) onAbort();
              else opts.signal.addEventListener("abort", onAbort, { once: true });
            }
          }
        } catch (e) {}
        return r;
      };
      doc.modelContext.executeTool = async function (name, input) {
        var args = normalizeInput(input);
        var cached = (seen !== null && Object.prototype.hasOwnProperty.call(seen, name)) ? seen[name] : null;
        if (cached !== null && typeof cached.then === "function") {
          try { cached = await cached; } catch (e) { cached = null; }
        }
        if (cached === null) {
          try { cached = await findHandle(name); } catch (e) { cached = null; }
          if (cached !== null) { try { seen[name] = cached; } catch (e) {} }
        }
        var target = cached !== null ? cached : name;
        var out;
        try { out = await origExec(target, args); }
        catch (e) {
          var m2 = String((e && e.message) || e);
          if (/origin|RegisteredTool/i.test(m2)) {
            var fb = null;
            try { fb = customTools[name] || null; } catch (e3) {}
            if (fb && typeof fb.execute === "function") return await fb.execute(args);
          }
          if (/not of type ['"]?object['"]?|Failed to parse input arguments/i.test(m2) && typeof input !== "string") {
            out = await origExec(target, JSON.stringify(args));
          } else throw e;
        }
        if (typeof out === "string") {
          try { return JSON.parse(out); } catch (e) { return out; }
        }
        return out;
      };
      Object.defineProperty(doc.modelContext, "__awLenient", { value: true, configurable: true });
      doc.modelContext.getTools = function () {
        // Merge upstream natives with locally registered tools (local wins
        // on collisions). Pre-existing native tools stay visible after mount.
        var cached = Object.keys(seen).map(function (n) {
          var t = seen[n];
          var c = { name: n };
          // Local description first: synchronous at register time, so the
          // listing is correct even while the handle promise is pending.
          if (Object.prototype.hasOwnProperty.call(localDesc157, n)) c.description = localDesc157[n];
          else if (t && typeof t.then !== "function" && typeof t.description === "string") c.description = t.description;
          return c;
        });
        var upstream;
        try { upstream = origList(); } catch (e) { upstream = null; }
        if (upstream === null) return Promise.resolve(cached);
        return Promise.resolve(upstream).then(function (list) {
          return mergeToolLists(cached, list);
        }, function () { return cached; });
      };
      ["registerTool", "getTools"].forEach(function (k) {
        if (typeof registry[k] === "function" && typeof doc.modelContext[k] !== "function") {
          try { doc.modelContext[k] = registry[k]; } catch (e) {}
        }
      });
    }
  } catch (e) {}
  try {
    if (typeof navigator !== "undefined" && !navigator.modelContext) {
      Object.defineProperty(navigator, "modelContext", { value: doc.modelContext, configurable: true });
    }
  } catch (e) {}

  var mc = doc.modelContext;
  // Late native: the page may define or replace document.modelContext AFTER
  // mount (hydration races, SPA re-renders). When the live context differs
  // from the captured one, list() merges both registries (local wins) and
  // call() falls through to the live context on unknown-tool. When they are
  // the same object — the normal path — behavior is untouched.
  function liveCtx() {
    try {
      var cur = doc.modelContext;
      if (cur && cur !== mc && typeof cur.getTools === "function") return cur;
    } catch (e) {}
    return null;
  }
  var facade = {
    list: function () {
      var live = liveCtx();
      var ours;
      try { ours = mc.getTools(); } catch (e) { ours = []; }
      if (!live) return ours;
      return Promise.resolve(ours).then(function (a) {
        var base = Array.isArray(a) ? a : [];
        var b;
        try { b = live.getTools(); } catch (e) { return base; }
        return Promise.resolve(b).then(function (bl) {
          return mergeToolLists(base, bl);
        }, function () { return base; });
      }, function () {
        try { return live.getTools(); } catch (e) { return []; }
      });
    },
    call: function (name, args) {
      var a = args === undefined ? {} : args;
      var live = liveCtx();
      var p;
      try { p = mc.executeTool(name, a); } catch (e) { p = Promise.reject(e); }
      if (!live || typeof live.executeTool !== "function") return p;
      return Promise.resolve(p).then(null, function (e) {
        if (/unknown tool/i.test(String((e && e.message) || e))) return live.executeTool(name, a);
        throw e;
      });
    },
    define: function (tool) { mc.registerTool(tool); return tool.name; },
  };
  try {
    if (!root.webmcp) Object.defineProperty(root, "webmcp", { value: facade, configurable: true, writable: true });
  } catch (e) { root.webmcp = facade; }
})();
