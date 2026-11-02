# Hosting WebMCP pages: minimal host research

Research date: 2026-10-30. Question: what is the SIMPLEST way to add WebMCP
(`document.modelContext` page-provided agent tools) support to a desktop app with an
embedded Chromium, given an Electron main process that owns its own `WebContentsView` +
preload (i.e. it CAN run pre-parse JS)? Context: our current host is ~400 lines
(registry, `toolchange` events, `fromOrigins` opt-in, permissions-policy gating,
per-frame IPC validation) and is suspected to be overcomplicated.

Every claim below cites the primary source that owns it. Spec citations are to the
Web ML Community Group draft (not a W3C Standard — stated on the document itself):
https://webmachinelearning.github.io/webmcp

---

## 0. Bottom line

- **For "list + call page tools", you do not need a JS host at all.** If your Chromium
  exposes the experimental CDP `WebMCP` domain, `WebMCP.enable` + the
  `toolsAdded`/`toolsRemoved`/`toolResponded` events + `WebMCP.invokeTool` give you
  discovery and invocation with **zero page JS**. This is exactly what agent-browser
  does (public docs, cited §5). In Electron the same protocol is reachable from the
  main process via `webContents.debugger` — no preload, no `contextBridge`, no
  per-frame IPC.
- **If CDP is unavailable/undesirable, a ~60-line preload host is enough** for real
  pages: expose nothing to the page; from the main world, call the page's own
  `document.modelContext.getTools()` / `executeTool()` via `executeJavaScript` (or a
  20-line `contextBridge` relay). Real pages (verified: Cubecade's production bundle,
  §3) register inside a React mount effect with `document.modelContext ||
  navigator.modelContext` feature-detection, so any host surface installed before page
  script runs is in time.
- **The ~400-line surface (registry mirror, `toolchange`, `fromOrigins`,
  permissions-policy gating, per-frame IPC validation) re-implements browser duties.**
  Keep it only if you must present a *spec-faithful `document.modelContext` to pages
  on a Chromium that lacks native WebMCP* — i.e. you are a polyfill, not just a host.
  Even then, start from the W3C polyfill (§3), don't hand-roll.

Ranked recommendation with trade-offs is in §6.

---

## 1. Spec minimum: what the draft actually requires

Source of truth: the draft, §§4.1–4.4 (API), §3 (supporting concepts), §5
(agent interaction). IDL quoted from the spec's IDL index
(https://webmachinelearning.github.io/webmcp — "IDL Index").

### 1.1 The minimal host surface is three methods on `document.modelContext`

```webidl
partial interface Document {
  [SecureContext, SameObject] readonly attribute ModelContext modelContext;
};

[Exposed=Window, SecureContext]
interface ModelContext : EventTarget {
  Promise<undefined> registerTool(ModelContextTool tool, optional ModelContextRegisterToolOptions options = {});
  Promise<sequence<RegisteredTool>> getTools(optional ModelContextGetToolOptions options = {});
  Promise<DOMString> executeTool(RegisteredTool tool, optional object inputObject, optional ModelContextExecuteToolOptions options = {});

  attribute EventHandler ontoolchange;
  attribute EventHandler ontoolactivated;
  attribute EventHandler ontoolcancel;
};
```

(https://webmachinelearning.github.io/webmcp — IDL Index; §4.1 "Each `Document`
object has an associated `ModelContext`… Upon creation of the `Document` object, its
associated `ModelContext` must be set to a new `ModelContext` object created in the
`Document`'s relevant realm.")

Tool and options dictionaries (same IDL index):

```webidl
dictionary ModelContextTool {
  required DOMString name;
  USVString title;
  required DOMString description;
  object inputSchema;
  required ToolExecuteCallback execute;   // Promise<any> (object inputObject, ToolExecuteCallbackOptions options)
  ToolAnnotations annotations;            // { readOnlyHint, untrustedContentHint, consequentialHint, debugging }, all default false
};
dictionary ModelContextRegisterToolOptions { sequence<USVString> exposedTo; AbortSignal signal; };
dictionary ModelContextGetToolOptions { sequence<USVString> fromOrigins; };
dictionary ModelContextExecuteToolOptions { AbortSignal signal; };
dictionary RegisteredTool {
  required DOMString name; DOMString title; required DOMString description;
  object inputSchema; required Window window; required USVString origin;
  ToolAnnotations annotations;
};
```

So **yes — the interop core is just `registerTool` / `getTools` / `executeTool` on
`document.modelContext`**. Everything else in the surface exists for multi-frame
routing and lifecycle, enumerated below.

### 1.2 What each remaining piece is, and whether list+call needs it

**`RegisteredTool.window` + `.origin` — REQUIRED for correct invocation.**
`getTools()` resolves to descriptors that carry the owning frame's `window` and
serialized `origin` (IDL above; §4.2 `getTools()` "Returns a promise that resolves
to a list of registered tools from this document and its descendants that are
exposed to this document"). `executeTool(tool, …)` "Executes a tool on the document
it was registered on" (§4.2, `document.modelContext.executeTool(tool, inputObject,
options)`). A host that flattens tools to `{name, description, inputSchema}` and
forgets which frame/document each came from **cannot route `executeTool` correctly
when two frames register the same name** — the draft's own race note calls out that
same-name re-registration across documents is *not* protected
(https://webmachinelearning.github.io/webmcp — §3, "unregistration-execution race"
note referencing issue #92). agent-browser's CLI learned this the hard way: it
requires `--frame <frame-id>` when frames collide
(https://agent-browser.dev/webmcp). **Minimal host rule: keep (name, frameId/origin)
with every tool.**

**`name` validation — REQUIRED if you accept registrations** (i.e. you implement
`registerTool` yourself rather than delegating to native Chrome). `registerTool`
rejects when: name is empty, longer than 128, or contains anything outside ASCII
alphanumeric + `_`, `-`, `.`; description is empty; `inputSchema` fails
`JSON.stringify` (throws `TypeError` or rethrows, e.g. circular refs); a tool with
that name already exists; the register-time `signal` is already aborted; the
document isn't fully active or isn't allowed `tools` (https://webmachinelearning.github.io/webmcp
— §4.2, `registerTool(tool, options)` method steps). If you host via CDP or via the
page's native `modelContext`, the browser enforces all of this for you.

**`inputSchema` serialization rules — REQUIRED for schema passthrough.**
The page passes a JS object; the implementation stores `JSON.stringify(inputSchema)`
(the "stringified input schema", empty string if omitted) and `getTools()` parses it
back ("`inputSchema`: the result of parse-a-JSON-string… if not the empty string;
otherwise undefined. Note: This will never throw")
(https://webmachinelearning.github.io/webmcp — §4.2 registerTool steps 11–12 and the
getTools construction steps). Consequence for a minimal host: **treat `inputSchema`
as opaque JSON** — never `JSON.parse` it yourself except to display; `executeTool`'s
`inputObject` defaults to a fresh empty object when omitted, and non-object JSON
input rejects with `DataError` ("invalid input",
https://webmachinelearning.github.io/webmcp — §3 tool-execution-error table).

**`exposedTo` (register) / `fromOrigins` (discover) — REQUIRED only for
cross-origin frames; pure overhead for same-origin/single-frame pages.**
Two-sided opt-in: a tool is visible to a calling document iff same-origin with the
owner, or the caller's origin is in the tool's `exposedTo` *and* the owner's origin
is in the caller's `fromOrigins` ("Tools from cross-origin documents are only
included if: 1. The hosting origin is listed in the `fromOrigins` option. 2. The
tool has been explicitly exposed to your origin."
https://developer.chrome.com/docs/ai/webmcp/imperative-api — "Discover tools").
Both lists accept secure origins only; untrustworthy origins reject with
`SecurityError` (https://webmachinelearning.github.io/webmcp — §4.2 registerTool
step 17 / getTools steps). **If you only host top-level same-origin pages, you can
delete both code paths.** Note the direction matters: `getTools({fromOrigins})`
*expands* the query set; it never *restricts* same-origin results
(https://webmachinelearning.github.io/webmcp — §4.2.4: "An empty list only includes
same-origin documents").

**Permissions-Policy `tools` gate — REQUIRED for spec-faithful registration, but it
is the *browser's* job, not the host's.** "Access to the APIs in this specification
is gated behind the policy-controlled feature `tools`, which has a default allowlist
of `'self'`" (https://webmachinelearning.github.io/webmcp — §4.4 "Permissions
policy integration"). `registerTool`/`getTools` reject with `NotAllowedError` when
the document isn't allowed to use `tools`; cross-origin iframes need
`<iframe allow="tools">` and `toolchange` notification skips disallowed documents
(https://webmachinelearning.github.io/webmcp — §3 "notify documents of a tool
change"; https://developer.chrome.com/docs/ai/webmcp/imperative-api —
"Permissions policy"). **A host that drives the page's native `modelContext` or CDP
gets this enforcement free.** You only re-implement it if you ship a from-scratch
`document.modelContext` in a browser without one — and getting it wrong (fail-open)
is a security hole, which is an argument for the CDP/polyfill options below.

**`toolchange` event — OPTIONAL for list+call (poll or subscribe instead).**
Fired at every same/allowed document's `ModelContext` on register/unregister
(https://webmachinelearning.github.io/webmcp — §3 "notify documents… to fire an
event named `toolchange`"; IDL `attribute EventHandler ontoolchange`). It is a
*page-side* discovery signal. A host can instead re-`getTools()` on navigation,
on a timer, or — with CDP — consume `toolsAdded`/`toolsRemoved` (agent-browser does
exactly this: "The daemon subscribes to CDP WebMCP events once per page session…
There is no per-action discovery polling or registration grace period",
https://agent-browser.dev/webmcp). The draft itself warns `toolchange` timing
relative to other tasks "cannot be relied upon" (same §3 algorithm note). Keep it
only if page-embedded (in-page iframe) agents must react live.

**`toolactivated` / `toolcancel` events (`ToolActivatedEvent`/`ToolCancelEvent`
with `toolName`) — NOT needed for list+call.** They fire on the *target* (tool
owner's) document when an execution starts / is cancelled, so the page can update
UI ("The browser signals that an AI agent executed a tool with the
`"toolactivated"` event… once form fields are pre-filled",
https://developer.chrome.com/docs/ai/webmcp/declarative-api; IDL in spec §4.3.1/§4.3.2).
They carry no data the *caller* needs (the caller learns outcomes from the
`executeTool` promise / CDP `toolResponded`). Drop both in a minimal host.

**`toolcancel` (execution cancellation) — OPTIONAL; `AbortSignal` unregistration is
REQUIRED semantics.** Two different signals: the *register*-time `signal` controls
tool lifetime ("There is no `unregisterTool` — pass `{ signal }` and abort it";
unregistration does *not* cancel in-flight executions,
https://webmachinelearning.github.io/webmcp — §3 "unregister a tool" note,
https://developer.chrome.com/docs/ai/webmcp/imperative-api — "Unregister tools").
Since Chrome 153, unregistering no longer cancels in-flight executions (same Chrome
page). The *execute*-time `signal` cancels one invocation
(`ModelContextExecuteToolOptions`, CDP `cancelInvocation`). A minimal host needs
unregistration to work (pages like Cubecade clean up with `() => t.abort()` — see
§3), but per-invocation cancel can be v1-omitted if your UX has no cancel button
(execution still resolves; you just ignore late results).

**`SecureContext` + `[SameObject]` — environment facts, not code.** The whole API
is `[SecureContext]` (HTTPS or localhost; file:// counts? — no: `file://` is a
secure context in practice for local files, but serve over localhost to be safe).
`document.modelContext` is `SameObject` (stable identity — safe to cache). The W3C
polyfill README states it plainly: "The API only exists in secure contexts, so
serve your page over HTTPS or from localhost"
(https://github.com/w3c-oss/webmcp-polyfill).

**Declarative API (`toolname`/`tooldescription`/`toolautosubmit` form attributes,
`SubmitEvent.agentInvoked`/`respondWith`, `:tool-form-active` CSS) — NOT in the CG
spec at all; Chrome-only surface.** Verified: the strings `toolname` (as an HTML
attribute) and `respondWith` do not occur in the spec text; they are documented only
on https://developer.chrome.com/docs/ai/webmcp/declarative-api. A JS host must NOT
re-implement this. Declarative tools still surface through the same discovery
channels (they are real registered tools), so list+call covers them with no extra
work when driven via native/CDP.

**Error taxonomy the caller observes** (§3 tool-execution-error table): `tool not
found → NotFoundError`, `invalid input → DataError`, `tool rejected →
OperationError`, `unserializable result → OperationError`, `target document unloaded
→ AbortError`. A minimal host maps these to error results; only the *names* cross
the boundary.

### 1.3 Spec-minimum checklist for "list + call"

| # | Piece | Verdict |
|---|-------|---------|
| 1 | `registerTool` with name/desc/inputSchema/execute (+title/annotations passthrough) | Only if you *implement* registration (polyfill path). Validation rules §4.2. |
| 2 | `getTools()` → `RegisteredTool{name,title,description,inputSchema,window,origin,annotations}` | REQUIRED (or CDP `toolsAdded` equivalent). Keep `window`/`origin`/frame identity. |
| 3 | `executeTool(tool, inputObject = {}, {signal})` → `DOMString` | REQUIRED (or CDP `invokeTool` equivalent). |
| 4 | Unregister = abort register-time signal | REQUIRED semantics (pages depend on it). |
| 5 | `exposedTo` / `fromOrigins` | Cross-origin only. Delete for same-origin. |
| 6 | Permissions-policy `tools` (default `'self'`) | Browser's job. Free via native/CDP; re-implement only in a from-scratch polyfill. |
| 7 | `SecureContext` | Environment fact (HTTPS/localhost). |
| 8 | `toolchange` | Optional for host (poll/CDP instead). Required only for in-page iframe agents. |
| 9 | `toolactivated` / `toolcancel` events | Page-side UX signals. Not needed for list+call. |
| 10 | Per-invocation cancel | Optional v1. |
| 11 | Declarative form API | Chrome-native only. Never re-implement in a host. |

Test oracle: the spec links the WPT suite at https://wpt.fyi/results/webmcp
(spec header "Test Suite"), and the W3C polyfill pins "the spec and WPT revisions
we test against" (https://github.com/w3c-oss/webmcp-polyfill — "Implementation status").

---

## 2. Native Chrome: CDP `WebMCP` domain

Protocol reference (tot): https://chromedevtools.github.io/devtools-protocol/tot/WebMCP/

### 2.1 Domain surface (enough to replace a JS host entirely)

Methods:

- `WebMCP.enable` — "Enables the WebMCP domain, allowing events to be sent.
  Enabling the domain will trigger a `toolsAdded` event for all currently
  registered tools." (i.e. enable = full snapshot + live stream; no polling.)
- `WebMCP.disable`
- `WebMCP.invokeTool({frameId, toolName, input})` → `{invocationId}` ("Response is
  sent before tool events.")
- `WebMCP.cancelInvocation({invocationId})`

Events:

- `WebMCP.toolsAdded {tools: Tool[]}` / `WebMCP.toolsRemoved {tools: RemovedTool[]}`
- `WebMCP.toolInvoked {frameId, input, invocationId, toolName}` (execution start)
- `WebMCP.toolResponded {invocationId, status, errorText?, exception?, output?}` —
  `status ∈ {Completed, Canceled, Error}`; "The output is untrusted and poses a
  prompt injection risk."

Types: `Tool{name, description, frameId, inputSchema, annotations?, backendNodeId?,
stackTrace?}` (note: declarative tools carry `backendNodeId`; imperative tools
don't need it); `RemovedTool{frameId, name}`; `Annotation{readOnly, untrustedContent,
consequential, debugging, autosubmit?}`.

### 2.2 Availability: flag, channel, version

- **Local dev flag:** `chrome://flags/#enable-webmcp-testing`
  ("Open Chrome and navigate to `chrome://flags/#enable-webmcp-testing`. Set the
  flag to Enabled. Relaunch Chrome" —
  https://developer.chrome.com/docs/ai/webmcp). Secondary corroboration:
  https://mcpservers.org/learn/chrome-webmcp, https://tds.s-anand.net/2026-09/week-3/webmcp/.
- **Origin trial from Chrome 149** ("Join the WebMCP origin trial from Chrome 149",
  https://developer.chrome.com/docs/ai/webmcp; trial announcement
  https://developer.chrome.com/blog/ai-webmcp-origin-trial, 2026-06-09; trial id
  `4163014905550602241`, `ot_chromium_trial_name: WebMCP` — ChromeStatus API
  `https://chromestatus.com/api/v0/features/5117755740913664`). A third-party
  writeup notes a developer trial from Chrome 146 and origin trial from 149
  (https://www.npmjs.com/package/webmcp-sdk — readme; secondary, treat 149 as the
  firm number).
- **ChromeStatus: still Proposed, not shipped** (`status: Proposed`,
  `finch_name: WebMCP`, devtrial flag name "Experimental Web Platform features" —
  same API response). So: gate on runtime detection (`document.modelContext`
  presence / CDP domain presence), never on a version number.
- **Behavioral version markers** (Chrome docs, useful when debugging old vs new):
  unregister-without-cancelling-in-flight changed "as of Chrome 153"; `debugging`
  annotation "available from Chrome 156"; "`window.ontoolactivated` and
  `window.ontoolcancel` are not supported from Chrome 156"
  (https://developer.chrome.com/docs/ai/webmcp/imperative-api,
  https://developer.chrome.com/docs/ai/webmcp/declarative-api).
- **CDP domain itself is experimental:** "older Chrome builds may not expose the
  experimental CDP WebMCP domain. In that case the command returns a
  `webmcp_unsupported` error" (https://agent-browser.dev/webmcp). So even the CDP
  path needs a capability check + fallback.

### 2.3 "Enable flag + drive CDP, zero page JS" — confirmed viable

agent-browser is the existence proof: "The daemon subscribes to CDP WebMCP events
once per page session… Initial subscription is bounded to one second… Asynchronous
registrations appear on the next normal browser response"
(https://agent-browser.dev/webmcp). Its CLI is `webmcp list [<tool> --frame …]
| invoke <tool> --params … [--detach] | result <invocation-id> | cancel
<invocation-id>` (https://agent-browser.dev/commands). It launches stock Chrome
with its own flags and opts out via `--no-webmcp` / `AGENT_BROWSER_NO_WEBMCP=1`
(https://agent-browser.dev/webmcp). In Electron, the transport equivalent is
`win.webContents.debugger.attach()` + `sendCommand('WebMCP.enable')` /
`sendCommand('WebMCP.invokeTool', …)` with `'message'` events — the `Debugger`
class is "an alternate transport for Chrome's remote debugging protocol" with
`attach` / `sendCommand(method[, commandParams])` / `'message'` event
(https://www.electronjs.org/docs/latest/api/debugger).

Caveats for the CDP-only path: you see **page-provided metadata only**
(names/descriptions/schemas/annotations are untrusted — agent-browser marks every
summary `untrusted: true` and warns metadata "cannot bypass host isolation",
https://agent-browser.dev/webmcp); invocation runs page JS in the page (you inherit
whatever the tool does — consequential actions need your own confirmation policy);
and per-frame routing needs the `frameId` (same doc).

---

## 3. Page-level polyfill: `<script>`/preload with no flags

### 3.1 The official polyfill exists — use it, don't write one

- **W3C pilot polyfill:** repo https://github.com/w3c-oss/webmcp-polyfill
  (redirects from `webmachinelearning/webmcp-polyfill`), `src/` + `tests/` + `wpt/`
  runner, types from `webmcp-types`. Usage: `import "@w3c/webmcp-polyfill/auto"`,
  or `installWebMCP()` from `@w3c/webmcp-polyfill`, or standalone
  `dist/polyfill.js` script tag before app code; SSR-safe; "Neither entry point
  replaces a native `document.modelContext`, even a partial one"
  (https://github.com/w3c-oss/webmcp-polyfill). Covers "registration, discovery,
  execution, cancellation and `toolchange`, including frame exposure and origin
  filtering" over `MessageChannel` (with 500 ms initial cross-frame discovery);
  **not** covered: "Declarative forms and the `toolactivated`/`toolcancel` events"
  — matching exactly the §1.2 optional list.
- **Registry status (accuracy warning):** as of 2026-10-30,
  `https://registry.npmjs.org/@w3c%2Fwebmcp-polyfill` returns **404 Not found** —
  the README's `npm install @w3c/webmcp-polyfill` does not currently resolve from
  the public registry. Vendoring from git or waiting for publication is required.
  (`webmcp-types` itself is published, latest 0.1.10 — same registry.)
- **Independent polyfills:** `webmcp-polyfill@0.0.2` ("A WebMCP browser polyfill",
  repo `github.com/ripulio/web-mcp`, no README, ~45 weekly downloads — hobby-grade,
  not a dependency to bet on); `@mcp-b/webmcp-polyfill@6.0.3` ("Vendored upstream
  WebMCP polyfill plus temporary declarative tools" — i.e. a forward-compatible
  vendor of the W3C work plus extras).

### 3.2 Timing: does a document_start-injected `document.modelContext` reach real pages?

Yes for the dominant real-world pattern. Evidence, strongest first:

1. **Cubecade production bundle** (https://cubecade.openai.chatgpt.site/,
   `assets/page-CTM8wbmX.js`, fetched 2026-10-30) registers inside a React mount
   effect with dual-API feature detection:
   ```js
   useEffect(() => {
     let e = document.modelContext || navigator.modelContext;
     if (!e?.registerTool) return;
     let t = new AbortController, ...
       r = n => { try { e.registerTool(n, { signal: t.signal }) } catch { e.registerTool(n) } };
     r({ name: 'get_cube_state', … }); r({ name: 'queue_cube_moves', … });
     return () => t.abort();   // AbortSignal-only unregistration, per spec
   }, []);
   ```
   Effects run after parse/hydration — any `document.modelContext` installed
   before page script executes (preload, `document_start` script, or
   `dist/polyfill.js` loaded before the app bundle, as the W3C README instructs)
   is visible here. The `|| navigator.modelContext` fallback also documents the
   **spec's own rename churn**: early-2026 material describes "the `modelContext`
   API, exposed on the `Navigator` interface"
   (https://abvijaykumar.medium.com/webmcp-web-model-context-protocol-agents-are-learning-to-browse-better-22fcefc981d7,
   Feb 2026); `webmcp-react@0.2.0` "realigns… API moved to `document.modelContext`"
   (https://github.com/agentcathq/webmcp-react); `use-webmcp-tool` is "built
   against the current WebMCP spec, which exposes the imperative API on
   `document.modelContext`" (npm `use-webmcp-tool@0.3.0`, repo
   `github.com/GoogleChromeLabs/use-webmcp-tool`). **A host/polyfill should accept
   both spellings on read** (check `document` first, fall back to `navigator` —
   exactly what Cubecade does) while registering itself only on `document`.
2. **Framework adapters all use the same late-registration pattern**, so the same
   timing conclusion holds: `useWebMCP` (VueUse, `@vueuse/core@15.0.0`)
   "feature-detects and degrades to a no-op… check `isSupported`"
   (https://vueuse.org/core/usewebmcp/); `webmcp-react` and `use-webmcp-tool`
   likewise no-op when absent.
3. **Content-script timing semantics** (for the extension-analogous case):
   `run_at: document_start` = "injected after any files from `css`, but before any
   other DOM is constructed or any other script is run"; `document_end` = after DOM
   complete; `document_idle` (default) = between those and `window.onload`
   (https://developer.chrome.com/docs/extensions/develop/concepts/content-scripts —
   "Run time"). Content scripts run in **isolated worlds** sharing only the DOM
   (same page) — the direct analog of Electron's preload isolation (Electron's own
   docs: `contextIsolation` "uses the same technique used by Chrome Content
   Scripts", https://www.electronjs.org/docs/latest/api/structures/web-preferences).

### 3.3 What breaks with a plain-script polyfill

- **Module-eval-time snapshots miss it.** `const mc = document.modelContext` at
  module top level in a `<script type="module">` (deferred to after parsing but
  ordered against other module scripts) or bundler-evaluated-at-import code that
  runs *before* your polyfill script executes will capture `undefined`. Mitigation
  is ordering (polyfill `<script>` before app bundles — the W3C README's Next.js
  guidance is exactly this: "Next.js runs client instrumentation before
  hydration", https://github.com/w3c-oss/webmcp-polyfill), not more code. A
  preload (which runs before *any* page script — §4) strictly dominates
  `document_start` injection here.
- **Cross-frame discovery needs the MessageChannel machinery.** Same-origin tools
  "including those in parents and siblings" require per-frame installation plus
  the 500 ms discovery window and origin-checked `MessageChannel` routing
  (https://github.com/w3c-oss/webmcp-polyfill — "Frames"). A single `<script>` in
  the top frame does NOT give you iframe tools. If you own the browser (Electron),
  CDP or per-frame preload injection is simpler than the polyfill's channel
  protocol.
- **Never overwrite native.** The polyfill contract is install-only-if-absent
  (same README). A native-but-partial `modelContext` (older Chromium) plus a
  polyfill that blindly overwrites it is worse than feature-detecting and
  degrading.
- **`SecureContext` still applies.** A polyfill can't conjure the API on
  `http://` origins and shouldn't try (callers must handle absence — every
  adapter above does).

---

## 4. Electron minimum

### 4.1 Smallest integration that lists + calls page tools

Preload runs "before other scripts run in the page" and "before a web page loads
in the renderer" (https://www.electronjs.org/docs/latest/tutorial/tutorial-preload:
"What is a preload script?", "Preload scripts are injected before a web page loads
in the renderer, similar to a Chrome extension's content scripts"). With
`WebContentsView`, `webPreferences` (including `preload`) are passed to
`new WebContentsView({ webPreferences })`
(https://www.electronjs.org/docs/latest/api/web-contents-view), and `preload` must
be an absolute path
(https://www.electronjs.org/docs/latest/api/structures/web-preferences —
"`preload`… The value should be the absolute file path to the script").

Two minimal shapes (pick one):

**A. Zero-page-surface relay (recommended if native WebMCP exists in your
Electron's Chromium):** preload exposes a tiny `contextBridge` API whose methods
run *in the page's main world* against the page's own native `modelContext`
(e.g. via `contextBridge.executeInMainWorld` or a closure relay), or — even
simpler — the main process calls `contents.executeJavaScript('document.modelContext
&& document.modelContext.getTools()')`
(`contents.executeJavaScript(code[, userGesture])`,
https://www.electronjs.org/docs/latest/api/web-contents). Registration stays 100%
page-native; validation, permissions-policy, and frame routing stay the browser's
job. Roughly 30–60 lines including IPC.

**B. CDP-driven, no preload at all:** `webContents.debugger.attach()` +
`sendCommand('WebMCP.enable')`, consume `'message'` events
(`toolsAdded`/`toolsRemoved`/`toolResponded`), invoke with
`sendCommand('WebMCP.invokeTool', {frameId, toolName, input})`
(protocol §2.1; transport
https://www.electronjs.org/docs/latest/api/debugger). Zero page JS, zero bridge
types, works for pages that snapshot the API early (§3.3) because nothing is
injected.

**C. Full-spec polyfill host:** only if your Electron's Chromium predates WebMCP
*and* you must serve a spec-faithful `document.modelContext` to pages. Then vendor
the W3C polyfill (§3.1) into the preload instead of hand-rolling the ~400 lines.

### 4.2 `contextIsolation: false` is never required

- Defaults: `contextIsolation` defaults to `true` ("This option should be used
  when loading potentially untrusted remote content",
  https://www.electronjs.org/docs/latest/api/structures/web-preferences);
  `sandbox` defaults to `true` since Electron 20
  (https://www.electronjs.org/docs/latest/tutorial/sandbox); preload under sandbox
  gets `require('electron')` (contextBridge/ipcRenderer), reduced `process`, and
  `global` — no Node builtins (same sandbox page).
- The designed crossing is `contextBridge.exposeInMainWorld(apiKey, api)` with
  `Function` values proxied and everything else copied+frozen; supported types are
  string/number/boolean/plain-Object/Array/Promise/Function/cloneables — **no
  prototypes, no symbols, no classes**
  (https://www.electronjs.org/docs/latest/api/context-bridge — "Parameter / Error /
  Return Type support"). That table is the whole design constraint for a relay:
  pass tool descriptors as plain JSON across, and proxy `execute` as an async
  function (or keep execution in-page and only ferry results).
- "Without context isolation, I used to provide APIs via `window.X = …`. Now?"
  → `contextBridge.exposeInMainWorld` (same isolated-context tutorial:
  https://www.electronjs.org/docs/latest/tutorial/context-isolation). Disabling
  isolation buys nothing for WebMCP hosting and forfeits the documented
  untrusted-content protection. Keep `sandbox:true, contextIsolation:true`.

### 4.3 What about `enableBlinkFeatures` / flags to force WebMCP on?

`webPreferences` offers `enableBlinkFeatures` ("A list of feature strings… The
full list… in `RuntimeEnabledFeatures.json5`",
https://www.electronjs.org/docs/latest/api/structures/web-preferences) and
`experimentalFeatures`. ChromeStatus names the dev-trial flag "Experimental Web
Platform features" with `finch_name: WebMCP` (ChromeStatus API, §2.2). The exact
Blink runtime-feature string for WebMCP in *your* Electron's Chromium should be
confirmed at runtime (check `RuntimeEnabledFeatures.json5` in that Chromium, or
probe `document.modelContext` with the flag on) — do not hard-code a guess from
this report. Runtime feature-detection (`if (!mc?.registerTool) return`, per
Cubecade §3.2) is the robust gate regardless.

---

## 5. Prior art

### 5.1 agent-browser: JS host → CDP host (the migration to copy)

- **Before (page-JS era):** agent-browser shipped a WebMCP *generation* skill that
  helps agents "expose existing page workflows as validated page tools"
  (CHANGELOG 0.36.0, https://github.com/vercel-labs/agent-browser/blob/main/CHANGELOG.md)
  — i.e. tooling to *author* page tools, still executed through the page.
- **After (native CDP era):** 0.36.0 "Added experimental WebMCP support for
  discovering and invoking tools provided by the current page, including
  frame-aware tool selection, detached results, cancellation" (same CHANGELOG);
  0.37.0 "Added WebMCP availability output so navigation responses advertise when
  a page exposes allowed WebMCP tools"; 0.38.0 "Added WebMCP catalog updates to
  normal browser responses. Concise, content-bounded summaries appear on initial
  discovery and catalog changes, while full schemas remain opt-in". Current docs:
  daemon subscribes to CDP WebMCP events once per page session, 1 s bounded
  initial subscribe, no polling; `webmcp list [<tool> --frame] / invoke --params
  / result / cancel`; `--no-webmcp` opt-out; `webmcp_unsupported` on Chromes
  without the domain (https://agent-browser.dev/webmcp). MCP-agent access is an
  opt-in command profile (`agent-browser mcp --tools core,webmcp`, same page).
- Lesson for us: they keep **bounded summaries by default, full schemas on demand**
  (16 tools / 4 KiB cap, `truncated: true`), treat all page metadata as
  `untrusted: true`, and route by frame — the three production concerns a minimal
  host must also handle, each in a few lines rather than a framework.

### 5.2 npm packages (each verified against the registry 2026-10-30)

| Package | Latest | What it is | Verdict |
|---|---|---|---|
| `use-webmcp-tool` | 0.3.0 | Chrome-maintained React hook (`repo: github.com/GoogleChromeLabs/use-webmcp-tool`), lifecycle-managed registration, MCP result normalization | Reference *page-side* pattern; not a host |
| `@mcp-b/react-webmcp` | 6.0.3 | React hooks for WebMCP | Page-side; alternative to above |
| `webmcp-react` | 1.3.1 | Zod-first React hooks + built-in polyfill, SSR-safe (`github.com/agentcathq/webmcp-react`) | Page-side; shows polyfill-in-library pattern |
| `@webmcp/react` | 0.2.1 | React bindings | Page-side |
| `@vueuse/core` (contains `useWebMCP`) | 15.0.0 | Vue composable, AbortSignal lifecycle, result normalization | Page-side; best-documented semantics (https://vueuse.org/core/usewebmcp/) |
| `webmcp-sdk` | 0.5.9 | "TypeScript toolkit… Built for `navigator.modelContext`" | **Outdated** (pre-rename API) — do not adopt |
| `webmcp-types` | 0.1.10 | TS types for the draft | Safe shared vocabulary |
| `webmcp-polyfill` | 0.0.2 | "A WebMCP browser polyfill" (`github.com/ripulio/web-mcp`), no README, ~45 dl/wk | Hobby-grade; do not depend on |
| `@mcp-b/webmcp-polyfill` | 6.0.3 | "Vendored upstream WebMCP polyfill plus temporary declarative tools" | Usable stopgap if W3C package unpublished |
| `@w3c/webmcp-polyfill` | **404 on registry** | Official pilot polyfill (repo https://github.com/w3c-oss/webmcp-polyfill) | Vendor from git; re-check registry later |

### 5.3 Minimal example hosts to crib from

- **First-party demos** (all register-then-call through the page API; good
  fixtures for testing a host): WebMCP zaMaker, Travel demo, Le Petit Bistro
  (https://developer.chrome.com/docs/ai/webmcp — "Demo"); React Chess
  (`get-board-state`/`make-move`/…), WebMCP Page Agent, WebMCP Bridge
  (https://github.com/GoogleChromeLabs/webmcp-tools/blob/main/AWESOME_WEBMCP.md).
  Cubecade (§3.2) is the best *production* fixture: two tools, dual-spelling
  detection, signal-registration with fallback, MCP-shaped results.
- **Example extension + page agent** (Chrome docs' reference consumers — read
  these before designing host UX): "Example WebMCP extension" and "WebMCP Page
  Agent demo" (https://developer.chrome.com/docs/ai/webmcp/imperative-api —
  "Discover tools"). They show the intended `getTools({fromOrigins})` →
  `executeTool(tool, args)` flow a host mirrors.

---

## 6. Recommendation (ranked)

Assumption: the plugin owns the `WebContentsView` (can set flags, preload, and use
`webContents.debugger`), and the goal is list + call for real pages like Cubecade.

### Option A — CDP-driven host, no page JS (~60 lines). DEFAULT CHOICE.

`debugger.attach()` → `WebMCP.enable` → cache `toolsAdded`/`toolsRemoved` →
`invokeTool({frameId, toolName, input})` → await `toolResponded` (with
`cancelInvocation` if you add a cancel button later). Capability-check once per
session; fall back to B on `webmcp_unsupported`-equivalent errors.

- You LOSE: support for Chromes without the CDP domain (then fall back to B);
  in-page (iframe-resident) JS agents' `toolchange` signals still work natively,
  but *your* host doesn't synthesize them (you don't need to — you have the
  events); declarative-tool `backendNodeId` targeting (irrelevant for invoke).
- You KEEP and get free: spec validation, permissions-policy gating, frame
  routing (`frameId`), SecureContext enforcement, unregistration semantics,
  cancellation protocol. ~340 of your ~400 lines evaporate.

### Option B — Preload relay over the page's native `modelContext` (~60 lines).

Preload (runs before page scripts,
https://www.electronjs.org/docs/latest/tutorial/tutorial-preload) exposes via
`contextBridge` either proxied `list/call` functions or just a readiness flag,
while the main process drives `document.modelContext.getTools()/executeTool()`
through `executeJavaScript` per frame, keeping `(name, origin/frame)` with each
tool. Feature-detect `document.modelContext || navigator.modelContext` exactly
like Cubecade does.

- You LOSE vs A: you re-assume polling/subscription for discovery (re-`getTools`
  on navigation + `toolchange` listener relay, or a small poll); early-snapshot
  pages are fine (preload beats all page scripts) but iframes need per-frame
  execution; you inherit page-realm structured-clone limits across the bridge
  (plain-JSON tool descriptors only,
  https://www.electronjs.org/docs/latest/api/context-bridge).
- You KEEP vs writing a registry: all validation/gating stays native. This is the
  right fallback when CDP attach is unavailable (e.g. DevTools already attached —
  note `debugger` detaches when "DevTools is invoked for the attached
  `webContents`", https://www.electronjs.org/docs/latest/api/debugger).

### Option C — Full-spec polyfill host (adopt, don't build).

Vendor https://github.com/w3c-oss/webmcp-polyfill into the preload for Chromes
too old for native WebMCP. Only path that serves a spec-faithful
`document.modelContext` (validation, `exposedTo`/`fromOrigins`, `toolchange`
fan-out) where none exists.

- You LOSE: simplicity (≈ the 400 lines you have now, now maintained by W3C —
  that maintenance burden is precisely why C ranks last); declarative forms and
  `toolactivated`/`toolcancel` remain unimplemented upstream (their words, §3.1 —
  and §1.2 shows you don't need them).
- Choose C only on evidence (probe shows no native `modelContext` in your pinned
  Electron), never preemptively.

### What to delete regardless

`fromOrigins`/`exposedTo` enforcement, permissions-policy checks, and per-frame
IPC validation **in host code** — unless you ship C, these duplicate the browser
(§§1–2). What stays in every option: keep frame/origin identity alongside each
tool; bound untrusted metadata before showing it to the model (agent-browser's 16
tools/4 KiB pattern, https://agent-browser.dev/webmcp); confirm consequential
actions in host UX (spec §6 mitigations,
https://webmachinelearning.github.io/webmcp — §6.4; Chrome tool-security guidance
https://developer.chrome.com/docs/ai/webmcp/secure-tools — URL as linked from the
docs hub, verify before citing in user-facing text).

### Suggested verification before landing

1. Probe your pinned Electron: `document.modelContext?.registerTool` presence
   with and without `enableBlinkFeatures`/flag candidates (§4.3), and
   `WebMCP.enable` via `debugger.sendCommand` — pick A or B on results, not on
   this report's guesses.
2. Fixture-test against Cubecade (`get_cube_state`/`queue_cube_moves`, §3.2) and
   one GoogleChromeLabs demo (§5.3): list → schema fetch → invoke → output, plus
   abort-cleanup (navigate away, re-list).
3. Re-check `npm view @w3c/webmcp-polyfill` before choosing C — registry 404 as of
   2026-10-30 (§3.1).

---

## 7. Runtime probe results (2026-10-30, Electron 44 / Chrome 152, OpenCode Desktop 2.2.0)

Probed live in the plugin-owned `WebContentsView` (temporary probe build, since reverted):

- **Native API**: `document.modelContext` AND `navigator.modelContext` both `undefined` on a fresh view created with `webPreferences: { enableBlinkFeatures: "DocumentModelContext" }` and no preload. Either the runtime-feature string is wrong (the binary only exposes the C++ symbols `kDocumentModelcontext` / `kWebMCPTesting`, never observed the Bliss string form) or the gate is a launch-time base::Feature that `enableBlinkFeatures` cannot flip. Either way: **not available to a pure plugin** — matches §4.3's warning.
- **CDP domain**: `webContents.debugger.attach("1.3")` + `sendCommand("WebMCP.enable", {})` **succeeds**. But with no JS API, pages cannot register tools, so the domain yields zero `toolsAdded` events — present but unusable without native registration.
- **Conclusion**: Options A and B from §6 are unavailable on this build. The shipped approach (own `WebContentsView` + minimal pre-parse host, per-frame registries, senderFrame-derived routing, storage rendezvous for the window↔server bridge) stands. Slimmed per §6 delete-list on the same date: removed permissions-policy gate, `exposedTo`/`fromOrigins`, `toolchange`/`toolactivated`/`toolcancel` page events, and per-frame IPC validation beyond `senderFrame` (preload 395→277 lines). Full showcase re-verified green after slimming (`get_cube_state` + `queue_cube_moves` on Cubecade).
