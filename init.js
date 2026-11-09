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
        if (native && native.unregisterTool) { var r = native.unregisterTool.call(native, name); unbindShorthand(name); emit(); return r; }
        var ok = tools.delete(name);
        if (ok) { unbindShorthand(name); emit(); }
        return ok;
      },
      getTools: function () {
        if (native) {
          // Opaque upstream: report what we cached at register time.
          // Tools registered before this script loaded are not visible here.
          return Array.from(handles.values()).map(function (t) {
            var c = { name: t.name };
            if (typeof t.description === "string") c.description = t.description;
            return c;
          });
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
            var p = Promise.resolve(r).then(function () { return findHandle(tool.name); }).then(function (h) {
              seen[tool.name] = h || tool;
              return seen[tool.name];
            }, function () { return tool; });
            seen[tool.name] = p;
            bindShorthand(tool.name);
            if (opts && opts.signal) {
              var onAbort = function () { try { delete seen[tool.name]; } catch (e) {} unbindShorthand(tool.name); };
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
        var names = Object.keys(seen);
        if (names.length > 0) {
          return Promise.resolve(names.map(function (n) {
            var t = seen[n];
            var c = { name: n };
            if (t && typeof t.description === "string") c.description = t.description;
            return c;
          }));
        }
        try { return origList(); } catch (e) { return Promise.resolve([]); }
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
  var facade = {
    list: function () { return mc.getTools(); },
    call: function (name, args) { return mc.executeTool(name, args === undefined ? {} : args); },
    define: function (tool) { mc.registerTool(tool); return tool.name; },
  };
  try {
    if (!root.webmcp) Object.defineProperty(root, "webmcp", { value: facade, configurable: true, writable: true });
  } catch (e) { root.webmcp = facade; }
})();
