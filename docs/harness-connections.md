# Harness connection paths for third-party site agents

Scope: through which concrete mechanisms a third-party **site agent** (a remote assistant exposed as an invocable tool, e.g. `ask_mintlify({question})` backed by a docs-site assistant reached over MCP / WebMCP / deep-link) can be connected INTO each major coding-agent harness, and what its publisher must ship for one-click install.
Method: primary sources only — official docs subtrees, first-party config schemas, spec text, and maintainer-owned repos. Every claim carries its source. Secondary write-ups were leads, never evidence.

Companion briefs (not duplicated here): tool-shape questions live in `docs/site-agent-verb-design.md`; market questions live in `docs/product-landscape.md`.

## 1. opencode (V2)

Sources: `https://opencode.ai/llms.txt` index; `https://opencode.ai/v2/docs/mcp-servers/`; `/v2/docs/skills/`; `/v2/docs/agents/`; `/v2/docs/plugins/`; `/v2/docs/config/`; `/v2/docs/permissions/`; `/v2/docs/policies/`; `/v2/docs/cli/acp/`; `/v2/docs/console/harnesses/`.

### 1.1 MCP server (primary path)

- Add: `opencode mcp add <name> --url <https-url>` in a project, or `--global` for all projects; local stdio via `opencode mcp add <name> -- <command…>` with repeated `--env` flags. Connection check: `opencode mcp list`. OAuth sign-in is interactive (`/mcps` panel or `opencode mcp auth <name>`); sign-out/rotation via `opencode mcp logout <name>` (source: mcp-servers doc, Management + Setup sections).
- Config shape (hand-editable): `opencode.json(c)` → `mcp.servers.<name>` with `type: "local"` (`command[]`, `cwd`, `environment` with `{env:VAR}` substitution — shell `$VAR` is NOT expanded) or `type: "remote"` (`url` absolute Streamable-HTTP endpoint, `headers`, `oauth: false | {client_id, client_secret, scope, callback_port, redirect_uri, auth_server_metadata_url}`, `timeout`, `protocol: legacy|auto|2026-07-28`, `disabled`, `codemode`). Schema is source of truth at `https://opencode.ai/config.json` (sources: mcp-servers doc; config doc Variables + MCP sections).
- OAuth: on by default for remote servers; discovery → PKCE → refresh; client identity order is pre-registered `client_id` → CIMD (document at `https://opencode.ai/oauth/opencode/client.json`, loopback redirects) → DCR; credentials stored **outside project configuration** (source: mcp-servers doc, OAuth + Client registration sections).
- Discovery/precedence: project `opencode.json(c)` and `.opencode/opencode.json(c)` merge upward; global `~/.config/opencode/`; system dir (`/etc/opencode`, macOS `/Library/Application Support`, Windows `%ProgramData%`) and MDM managed preferences outrank everything (source: config doc, Locations).
- Tool surfacing: tool names are normalized `<server>_<tool>`; MCP prompts become slash commands `<server>:<prompt>`; default Code Mode groups tools per server (`codemode: false` exposes them natively); per-tool gating via `permissions` (`<server>_<tool>`) and MCP `instructions` are appended to the system prompt (source: mcp-servers doc, Names / Permissions / Resources / Server instructions sections).
- Limits: `policies` (`mcp.use`, `skill.use`, `plugin.use`) hard-deny and outrank project config; managed/Console policy cannot be lifted by a repo (source: policies doc). Sandboxing note: `shell` runs with the host user's authority and scanner-based inference is best-effort — prefer narrow allowlists (source: permissions doc, Scanner section). Experimental `subagent_depth` defaults to 1, so a subagent cannot spawn its own subagent unless raised (source: config doc, Experimental).

### 1.2 Skill

- Shape: `SKILL.md` at `.opencode/skills/<id>/` (directory form preferred; flat `<id>.md` at source root also loads) with `description` (required for model discovery) and optional `metadata.opencode/autoinvoke: false` to hide from the model while keeping explicit `@<id>` loading. Supporting files live beside `SKILL.md`; paths inside are relative to it (source: skills doc, Create/Discovery/Frontmatter/Loading).
- Discovery order (later wins): built-ins → `~/.claude/skills` → `~/.agents/skills` → `~/.config/opencode/skills` → project `.opencode/skills` (root→cwd) → explicit `skills[]` config entries, which also accept **HTTP catalogs** (`index.json` with `name/version/files`, same-origin, version-bumped refresh) (source: skills doc, Sources/Catalogs/Precedence).
- Gating: `skill` permission action by ID (`allow/ask/deny`, global or per-agent) (source: skills doc + permissions doc; permissions doc lists `skill` among built-in actions).

### 1.3 Agent definition

- Shape: Markdown `.opencode/agents/<name>.md` (nested path becomes `team/name` ID) or `agents` map in config; frontmatter takes `description`, `mode: primary|subagent|all`, `model`, `system` (body), `permissions[]`, `steps`, `hidden`, `color`, `disabled` (source: agents doc).
- Key limit for site-agent publishers: a parent agent's `subagent` permission rules decide which child IDs it may launch (`{action: subagent, resource: <id>, effect: allow|deny}`, last match wins); the child then runs under **its own** permissions, not a subset of the parent's (source: agents doc, Modes; permissions doc, Agents).

### 1.4 Plugin

- Shape: `plugins[]` entries — npm names (optionally versioned), git specs, or local directories — plus auto-discovery of `.ts`/`.js` files and package dirs under every discovered `.opencode/plugins/` (global `~/.config/opencode/plugins/`). TUI-only plugins go in `cli.json`. Management: `opencode plugin add|list|check|update|remove`. Two built-ins (`opencode.config.policy`, `opencode.provider.opencode`) ignore removal (sources: plugins doc, all sections).
- Publisher implication: a plugin can inject tools, hooks, agents, and more (build side at `/v2/docs/build/plugins/`), but it is code that runs in the user's process — trust bar is highest here.

### 1.5 ACP (opencode as agent)

- `opencode acp` runs OpenCode as an **ACP agent** (protocol v1, stdio JSON-RPC) for ACP clients such as Zed (`command: opencode, args: [acp]`). Auth reuses OpenCode's existing provider credentials (`opencode auth login`; `opencode-login` method, terminal or `_meta` variants). The client's `cwd` selects the OpenCode directory whose config/models/agents/skills/plugins/MCP apply (source: cli/acp doc, all sections).
- ACP-carried MCP: a client may pass stdio and HTTP servers on session setup; OpenCode adds them to the session's directory, passes HTTP headers through, does **not** do OAuth for them, does not advertise SSE, and does not support MCP-over-ACP (source: cli/acp doc, MCP servers section).
- Directionality note: this makes OpenCode *drivable by* ACP clients; it is not a path for a site agent to be installed *into* OpenCode sessions.

### 1.6 Publisher one-click story

- Keyless public MCP: `opencode mcp add <site> --url <https-mcp-url>` (one command) or a documented `mcp.servers` JSONC snippet; OAuth variant needs the interactive `/mcps` sign-in (DCR/CIMD, no publisher-side per-user work).
- Skill: ship a directory with `SKILL.md`; user drops it in `.opencode/skills/` (project) or `~/.config/opencode/skills/` (global), or publisher hosts an HTTP skill catalog referenced in `skills[]`.
- No first-party marketplace: distribution is npm/git/direct-path (plugins) or file/URL (skills, MCP config); organization push goes through managed config or the OpenCode Console workspace (Console MCP server at `https://opencode.ai/console/mcp`, OAuth+DCR, per-harness `add`+`login` commands — e.g. the Pi page shows `pi mcp add opencode-console --url …` / `pi mcp login`) (sources: plugins doc; console/harnesses doc; console/harnesses/pi doc).

## 2. Claude Code

Sources: `https://code.claude.com/docs/llms.txt` index; `https://code.claude.com/docs/en/mcp.md`; `/en/mcp-quickstart.md`; `/en/skills.md`; `/en/sub-agents.md`; `/en/hooks.md`; `/en/plugins/install.md`; `/en/plugins/components.md`; `/en/memory.md`; `/en/settings.md`.

### 2.1 MCP server (primary path)

- Add: `claude mcp add --transport http <name> <url>` (recommended), `--transport sse` (deprecated), stdio `claude mcp add <name> -- <cmd…>` with `--env KEY=val`, header auth via `--header`, or `claude mcp add-json`. Scopes: `--scope local` (default; `~/.claude.json` project entry, private) | `user` (`~/.claude.json` top-level `mcpServers`, all projects) | `project` (`.mcp.json` at root, committed, **requires per-clone approval** via workspace-trust dialog) (sources: mcp.md; mcp-quickstart.md Where-saved / Scope / Edit-.mcp.json sections).
- Entry shape: `{type: http|sse|stdio|ws, url|command, args, env, headers, headersHelper, timeout}`; a `url` without `type` is a config error; `ws` is header-auth-only (no OAuth, no `--transport` flag); `sdk` type is SDK-host-only and skipped by the CLI (source: mcp.md, Options 1–4 + warnings).
- Auth/credential custody: interactive browser OAuth from `/mcp` (`claude mcp login`); `remove` deletes stored tokens/client registration but does **not** revoke at the authorization server (`logout` first to revoke where supported); static tokens via `headers`; dynamic per-connect credentials via `headersHelper`; `${VAR}` expansion with warnings for unset vars and hidden-whitespace checks (sources: mcp.md, Managing/Auth/Configuration-warnings sections).
- Surfacing: tools `mcp__<server>__<tool>`; MCP prompts become `/` commands; resources via `@` mentions; tool-search loads on demand; per-server `timeout`, `MCP_TOOL_TIMEOUT`, output caps; enterprise allow/deny managed MCP policy can hide or block servers (sources: mcp.md, Tool-availability/Timeouts sections; settings.md managed-settings coverage).
- Limits: project `.mcp.json` servers stay `Pending approval` until trusted; `disabledMcpServers`/`disabledMcpjsonServers` scoping; cloud sessions read `.mcp.json` only for single-repo sessions outside project threads (sources: mcp-quickstart.md Troubleshooting; mcp.md Server-status; settings.md Cloud-sessions).

### 2.2 Skill (Agent Skills standard)

- Shape: `.claude/skills/<name>/SKILL.md` (also `~/.claude/skills/`, nested per-directory, `--add-dir`, plugin-bundled as `/<plugin>:<skill>`, claude.ai-synced). Frontmatter: `name`, `description` (+`when_to_use`), `disable-model-invocation`, `user-invocable`, `allowed-tools`/`disallowed-tools`, `model`/`effort`, `context: fork` (+`agent`, `background`), `hooks`, `paths`, `shell`, `metadata`, `license`, `compatibility` (sources: skills.md, Locations/Frontmatter-reference/Plugin/Configure sections).
- Publisher note: `allowed-tools` grants are permission-gated; `disable-model-invocation: true` makes the skill explicit-invoke only (source: skills.md).

### 2.3 Subagent (agent definition)

- Shape: `.claude/agents/*.md` (project, committed) or `~/.claude/agents/` (personal), managed-settings deployments, one-session `--agents` JSON, plugin `agents/` dirs. Frontmatter `name` + `description` required; plus `tools`/`disallowedTools` (incl. `mcp__<server>__*` scoping), `model`, `permissionMode`, `maxTurns`, `skills`, `mcpServers` (**inline or by reference** — scoped to the subagent), `hooks`, `memory`, `isolation: worktree`, `effort` (sources: sub-agents.md, Scope/Write-files/Frontmatter/MCP-servers sections).
- Limits relevant to publishers: plugin subagents **ignore** `hooks`, `mcpServers`, `permissionMode` (ship those at plugin level instead); background subagents lose most built-ins; sibling `mcp__reports__v2`-style double-underscore names need exact `mcp__<whole-name>__*` entries; spawning is via the `Agent` tool, restrictable with `Agent(name)` allowlists (sources: sub-agents.md, Available-tools/Restrict sections).

### 2.4 Plugin + marketplace (one-click path)

- Install UX: `/plugin install <name>@<marketplace>` (review pane → scope user/project/local) or `claude plugin install … --scope …`; marketplaces added from GitHub `owner/repo`, any git URL, local path, or hosted `marketplace.json` (`/plugin marketplace add`); official + community marketplaces; auto-update per marketplace (sources: plugins/install.md).
- Plugin contents (manifest `.claude-plugin/plugin.json`, only `name` required): `skills/`, `commands/`, `agents/` (scoped `<plugin>:<name>`), `hooks/hooks.json`, `.mcp.json` (servers appear as `plugin:<plugin>:<server>`, tools `mcp__plugin_<plugin>_<server>__<tool>`), `bin/`, LSP, output styles, monitors; `userConfig` prompts users for values with `sensitive: true` going to secure storage (never `settings.json`); org distribution via managed settings (sources: plugins/components.md; plugins/install.md).
- Publisher one-click: publish a marketplace (git repo + `marketplace.json`) containing a plugin that bundles the site MCP (`.mcp.json`) + a skill teaching when/how to call it + `userConfig` for the API key. User flow is one install command + config dialog + OAuth/Connect click.

### 2.5 Instructions / memory (secondary surface)

- `CLAUDE.md` (user/project/local/managed scopes) plus `AGENTS.md` auto-read when no `CLAUDE.md` is present (v2.1.277+, `Project instructions` setting), and path-scoped `.claude/rules/*.md` (source: memory.md). A site publisher cannot rely on these (user-owned files), but documenting a 3-line snippet is cheap fallback.

### 2.6 ACP

- Claude does not natively serve ACP; it is reachable from ACP clients through adapters (ACP agents page lists "Claude Agent" via Zed's SDK adapter, `github.com/zed-industries/claude-agent-acp`) (source: `https://agentclientprotocol.com/get-started/agents.md`).

## 3. Cursor

Sources: `https://cursor.com/docs/mcp.md`; `/docs/mcp/install-links.md`; `/docs/skills.md`; `/docs/rules.md`; `/docs/plugins.md`; `/docs/cloud-agent.md`; `/docs/api.md`; `/docs/cli/acp.md`; `/docs/llms.txt` sitemap.

### 3.1 MCP server (primary path)

- Config: project `.cursor/mcp.json` or global `~/.cursor/mcp.json`, `mcpServers` map. stdio entries take `type: "stdio"`, `command`, `args`, `env`, `envFile`; remote entries take `url` + `headers` (source: mcp.md, Using-mcp.json / STDIO-configuration).
- Auth/credential custody: secrets via env interpolation (`${env:NAME}`, plus `${userHome}`, `${workspaceFolder}`); OAuth via authorization-code+PKCE against the server's RFC 9728 metadata with DCR as public client; **static** OAuth client credentials via `auth: {CLIENT_ID, CLIENT_SECRET?, scopes[]}` when the provider needs allowlisted redirects; fixed redirect URIs the publisher must allowlist: `https://www.cursor.com/agents/mcp/oauth/callback` (web/agents) and `http://localhost:8787/callback` (desktop), multiplexed by `state` (source: mcp.md, Static-OAuth / Authentication sections).
- Management surfaces: install/toggle in Customize; team distribution via Dashboard → Plugins & MCPs (team MCP servers, Default team marketplace, install modes Default Off/On/Required); enterprise MCP allowlist (command/URL patterns, per-tool allowlists, network sandbox modes); **Cloud Agents keep their own MCP sign-ins**, separate from local (source: mcp.md, Team-distribution / Enterprise / FAQ sections; cloud-agent.md MCP-support).
- One-click: `cursor://anysphere.cursor-deeplink/mcp/install?name=<name>&config=<base64-of-mcp.json-entry>` links that prompt install (source: mcp/install-links.md). Marketplace listing is the groomed path (manual review) vs `cursor.directory` for community (source: mcp.md).

### 3.2 Skill

- Shape/locations: `.agents/skills/` + `.cursor/skills/` (project, incl. nested monorepo dirs that auto-scope), `~/.agents/skills/` + `~/.cursor/skills/` (user); compat reads `.claude/skills/` and `.codex/skills/`. `SKILL.md` **requires** `name` (== folder) + `description`; optional `paths` scoping, `disable-model-invocation`, `icon`/`color` (Custom Modes), `metadata` (source: skills.md).
- Distribution: personal `~/.cursor/skills/` sync to Cloud Agents (opt-in, that folder only); publish-to-team packs the skill into a plugin on the Default team marketplace (opt-in installs); repo import **only** via plugin+marketplace, never bare (sources: skills.md, Sync/Publish/Installing-from-repository).

### 3.3 Rules / agents / plugins

- Rules: `.cursor/rules/*.mdc` (typed: Always/Intelligent/Globs/Manual; plain `.md` ignored), nested `AGENTS.md`, Team rules (dashboard, precedence Team → Project → User), User rules (source: rules.md).
- Plugins bundle rules + skills + **agents** + commands + MCP + hooks; two formats: Agent Plugins (root `plugin.json`, portable skills+MCP) and Cursor Plugins (`.cursor-plugin/plugin.json`, + rules/agents/commands/hooks/variables); Cursor expands `${CURSOR_PLUGIN_ROOT}`, not the standard's `${PLUGIN_ROOT}`/`${PLUGIN_DATA}`; local dev in `~/.cursor/plugins/local` (gated by admin on Enterprise); team marketplaces distribute both formats (source: plugins.md).
- Programmatic side: Extension API `vscode.cursor.mcp.registerServer()`; **Cloud Agents API** (beta, team plans) to create/manage agents; TypeScript/Python SDKs + SDK Bridge (sources: mcp.md Extension-API; api.md Available-APIs/Authentication).

### 3.4 ACP (Cursor as agent)

- `agent acp` runs Cursor's CLI agent over stdio JSON-RPC: `initialize` → `authenticate (cursor_login)` → `session/new|load` → `session/prompt`, with `session/request_permission` (allow-once/allow-always/reject-once) and Cursor extension methods (`cursor/ask_question`, `cursor/create_plan`, `cursor/update_todos`, `cursor/task`, `cursor/generate_image`). MCP comes from project/user `.cursor/mcp.json`; **team-level MCP is not supported in ACP mode** (source: cli/acp.md).

## 4. Codex CLI (and Codex surfaces)

Sources: `https://developers.openai.com/codex/cli/`; `/codex/configuration/`; `/codex/config-file/*` (via configuration index); `/codex/agent-configuration/agents-md/`; `/codex/agent-configuration/subagents/`; `/codex/extend/mcp/`; `/codex/skills-and-plugins/`; `/codex/build-skills/`; `/codex/hooks/`; `/codex/webmcp/`.

### 4.1 MCP server (primary path)

- Config: `~/.codex/config.toml` (and project `.codex/config.toml`, trusted projects only; desktop app, CLI, IDE share it) → `[mcp_servers.<name>]`: stdio (`command` required, `args`, `env`, `env_vars` incl. remote-source entries, `cwd`) or Streamable HTTP (`url`, `auth: oauth|chatgpt`, `bearer_token_env_var`, `http_headers`, `env_http_headers`, `http_headers_helper` — a local command printing a JSON header object, refreshed on 401/403; no helper for remote-exec stdio). Tuning: `startup_timeout_sec`, `tool_timeout_sec`, `enabled`, `required`, `enabled/disabled_tools`, per-tool `approval_mode` + `output_token_limit`. CLI: `codex mcp add`, `codex mcp login <name>`, `/mcp` in composer (source: extend/mcp.md, Connect-Codex / config.toml-examples).
- OAuth/credential custody: CIMD → DCR → pre-registered `client_id` (`--oauth-client-id`, secret from env into `mcp_servers.<name>.oauth.client_secret`); loopback callback `http://127.0.0.1/callback` with documented server-specific callback-ID derivation rules, `mcp_oauth_callback_port/url` overrides; `scopes` in config or per-login; headers helper output cached per connection (source: extend/mcp.md, OAuth-registration/callback sections).
- Plugin-bundled servers: user config under `plugins."<plugin>".mcp_servers.<server>` controls enable/tool policy; plugin HTTP manifests use camelCase `clientId/callbackUrl/callbackPort` in `.mcp.json` (source: extend/mcp.md, Plugin-provided section).
- Server instructions: Codex reads MCP `instructions` at init as cross-tool guidance (keep first 512 chars self-contained) (source: extend/mcp.md, Supported-features).

### 4.2 Skill

- Shape: directory with `SKILL.md` (`name` + `description` required) + optional `scripts/`, `references/`, `assets/`, `agents/openai.yaml` (UI metadata, `policy.allow_implicit_invocation` default true, `dependencies.tools[]` can declare an MCP dependency with transport+url). Discovery: `$CWD/.agents/skills` → ancestors → `$REPO_ROOT/.agents/skills`, `$HOME/.agents/skills`, `/etc/codex/skills` (admin), bundled system; symlinks followed; 2%-of-context (≤10k-token cap via `skills.max_context_tokens`) listing budget; `$`-mention or implicit match (sources: build-skills.md).
- Distribution: author locally; distribute via **plugins**; install curated ones with `$skill-installer`; per-skill disable via `[[skills.config]]` (source: build-skills.md).

### 4.3 AGENTS.md / subagents / hooks

- `AGENTS.md` chain: `~/.codex/AGENTS.md` (or `AGENTS.override.md`) → per-directory files root→cwd (one per dir, skips empties, `project_doc_max_bytes` 32 KiB default, `project_doc_fallback_filenames` for alternate names) (source: agent-configuration/agents-md.md).
- Subagents: `~/.codex/agents/*.toml` or `.codex/agents/*.toml` with `name`/`description`/`developer_instructions` (+`model`, `model_reasoning_effort`, feature flags, skill disables); they **inherit** parent permissions/provider/MCP — a custom file cannot override `mcp_servers`/sandbox; built-ins `default/worker/explorer`; `[agents]` globals (source: agent-configuration/subagents.md).
- Hooks: `hooks.json` or inline `[hooks]` next to any active config layer; events incl. `PreToolUse`, `PermissionRequest`, `PostToolUse`, `SessionStart/End`, `SubagentStart/Stop`, `Stop`, `UserPromptSubmit`; handlers `command` + `mcp_tool` (`prompt`/`agent` parsed but skipped); project hooks need trust (`/hooks` review; `--dangerously-bypass-hook-trust` for automation); plugin-bundled `hooks/hooks.json` with `PLUGIN_ROOT`/`PLUGIN_DATA` (+Claude-compat vars) (source: hooks.md).

### 4.4 Plugins + WebMCP (the two one-click-adjacent paths)

- Plugins: universal directory shared by ChatGPT and Codex; package skills + MCP servers (+ hooks/lifecycle); build via Plugin Creator / Sites hosting; submit flow for public listing (sources: skills-and-plugins.md; extend/mcp.md Build-path; build-skills.md Distribute-with-plugins).
- **Site tools (WebMCP)**: the page-bound alternative to MCP — a website registers tools in the top-level page via `document.modelContext.registerTool({name, description, inputSchema, annotations, execute})`; the ChatGPT desktop built-in browser lets ChatGPT Work/Codex discover and call them in the live signed-in page, with per-invocation safety review. No install, but also no persistence (navigation kills availability); current subset excludes declarative (HTML-form) registration and iframe-registered tools; Enterprise needs Site-tools workspace policy; not in Edu (source: webmcp.md).

## 5. Pi (earendil-works/pi terminal agent)

Sources: `https://github.com/earendil-works/pi` (README); raw `packages/coding-agent/docs/extensions.md`; raw `…/skills.md`; raw `…/packages.md`; `https://opencode.ai/v2/docs/console/harnesses/pi/`; ACP agents/registry pages.

- Nature: minimal extensible harness; ships **no built-in permission system** (runs with user/process authority — containerize: Gondola/Docker/OpenShell patterns) and **no subagents or plan mode** by default (sources: README Permissions & Containerization; README intro).
- Extension (the MCP/skill-adjacent path): TypeScript modules in `~/.pi/agent/extensions/` (global) or `.pi/extensions/` (project-local, loads only after project trust), `pi -e path` for trials, `/reload` hot-reload. API: `pi.registerTool()`, `pi.registerCommand()`, event interception (`tool_call` can block/mutate, incl. permission-gate and path-protection examples), `resources_discover` (contribute skill/prompt/theme paths), `mcp_servers_change`, session/UI/persistence hooks; settings `packages`+`extensions` arrays; npm deps supported (sources: extensions.md, all cited sections).
- MCP: extensions register MCP servers (key capability + `mcp_servers_change` event); CLI supports `pi mcp add <name> --url …` + `pi mcp login` (Console-MCP page demonstrates the flow against `https://opencode.ai/console/mcp`) (sources: extensions.md; console/harnesses/pi doc).
- Skill: Agent Skills spec; `SKILL.md` directories under user/project skills dirs plus `~/.agents/skills`, `.agents/skills` (repo-ancestor walk); portable frontmatter (`name`, `description`, `license`, `compatibility`, `metadata`, `allowed-tools`, `disable-model-invocation`); `/skill:name` forcing; name-collision keeps first + warns (source: skills.md).
- Distribution (closest to marketplace): **Pi packages** — npm (`pi-package` keyword → gallery at `pi.dev/packages`), git, or local path; conventional `extensions/ skills/ prompts/ themes/` dirs or explicit `pi` manifest with globs; `pi install <source>`, `pi list/remove`, `pi update --extensions`; personal (`~/.pi/agent/settings.json`) vs project (`.pi/settings.json`, post-trust) scope with resource filters (sources: packages.md).
- Also: interactive TUI, `print`/JSON automation modes, RPC, TypeScript SDK (source: README Getting-started/packages list).
- ACP: **not native** — third-party `pi-acp` adapter (`github.com/svkozak/pi-acp`); registry lists "pi ACP" as the installable entry (sources: ACP agents page Pi entry; ACP registry page pi-ACP card).

## 6. Goose (AAIF/Linux Foundation; docs moved to goose-docs.ai)

Sources: `https://goose-docs.ai/docs/getting-started/using-extensions`; `…/guides/context-engineering/using-skills`; `https://github.com/aaif-goose/goose` (README: provider/extension counts, ACP); ACP agents page Goose entry.

- Extensions **are** MCP servers (plus built-ins). Built-ins always available: Developer (enabled by default — autonomous file/shell access; gate with permission modes + tool permissions), Computer Controller, Memory, Tutorial, Auto Visualiser; platform extensions: Analyze, Apps, Chat Recall, Code Mode, Extension Manager (dynamic enable), **Skills**, **Summon (subagents)**, Todo, Top of Mind. Built-in MCP servers are reusable from any agent (source: using-extensions.md, Built-in sections).
- Add paths: in-app directory UI; CLI (`goose session --with-extension "<env?>cmd…"`, `--with-builtin`, `--with-streamable-http-extension <url>`); **deeplinks** `goose://extension?cmd=<jbang|npx|uvx|goosed|docker>&arg=…&id=&name=&description=`; hand-edited `~/.config/goose/config.yaml` `extensions:` (`type: stdio|streamable_http`, `cmd/args/envs/timeout`, `uri`, `client_id`, `client_secret_key`, `scopes`) (source: using-extensions.md, Adding/Deeplinks/Config/Starting-session sections).
- OAuth/credential custody: remote HTTP extensions auto-negotiate (CIMD or DCR); pre-registered clients via `client_id` (+ `client_secret_key` naming a secret from `envs`/`env_keys`/secret store — value never in config; takes priority); `scopes` override; loopback callback on `127.0.0.1` ephemeral port (`GOOSE_OAUTH_CALLBACK_PORT` for pinned-port providers); per-request header scripts are a Devin thing, not Goose's — Goose uses static env-passed creds (source: using-extensions.md, Remote-extensions-with-pre-registered-OAuth-client).
- Skills: Agent Skills; `~/.agents/skills/`, `.agents/skills/`, plugin dirs `~/.agents/plugins/<plugin>/`; compat `.goose/skills`, `.claude/skills`; `SKILL.md` requires `name`+`description`; plugin namespacing `plugin:skill`; `/skills` listing (source: using-skills.md).
- ACP: listed as an ACP-speaking agent (agents page links its `docs/guides/acp-clients`; registry card points at `block/goose`) (sources: ACP agents page; registry page).

## 7. Devin (Cognition; cloud sessions + Devin CLI)

Sources: `https://docs.devin.ai/llms.txt` index; `/get-started/devin-intro.md`; `/cli/index.md` (+ quickstart); `/work-with-devin/mcp.md`; `/cli/acp/zed.md` (representative ACP page); ACP registry page Devin card.

- Cloud MCP: Customize → MCPs at Personal/Organization/Enterprise scope; recommended route is **marketplace plugin** (usually one MCP server + teaching skills; install at org scope → available to everyone), else org-admin **Add custom MCP** (STDIO/SSE/HTTP; HTTP preferred, SSE legacy). Auth per server: None | Auth Header | OAuth (DCR/CIMD auto, or own client ID/secret with provider-allowlisted fixed callback `https://api.devin.ai/mcp/oauth/callback` — same for all deployments incl. dedicated/custom-domain). **Access** decides custody: Organization = one shared connection (service account strongly recommended); Personal = per-member OAuth. STDIO has no Test-tools (session-only); adding custom servers needs Manage-MCP-Servers permission; CLI uses local config + `devin mcp login` (source: work-with-devin/mcp.md, all sections).
- Adjacent extension points: Skills (`SKILL.md` in repos or Customize page), plugins (personal/org/enterprise + team marketplace/enterprise ecosystem), Knowledge, Playbooks (+macros), Rules/`AGENTS.md` (16 KiB auto-injection cap noted in index), Secrets/Site-cookies/TOTP, Security Profiles (network/MCP/git scoping), hooks (Customize lists hooks alongside plugins/skills/rules; CLI documents lifecycle hooks), automations/schedules (sources: llms.txt index entries; mcp.md Customize-tab note).
- Devin CLI (local agent): rules, skills, plugins, subagents, MCP, hooks; config import from Cursor/Windsurf/Claude Code/Copilot/OpenCode/Zed; runs inside JetBrains/Zed/Xcode **as an ACP agent** (registry: "Devin CLI coding agent") (sources: cli quickstart + Extensibility section in llms.txt; cli/acp/zed.md; registry page).
- Inverse direction (do not confuse): the Devin API and the official Devin MCP server let *external tools drive Devin* — the reverse of this brief's site-agent-into-harness direction (source: llms.txt API + Devin-MCP entries).

## 8. ACP as an emerging standard

Sources: `https://agentclientprotocol.com/llms.txt` index; `/protocol/v1/overview.md`; `/protocol/v1/transports.md`; `/get-started/agents.md`; `/get-started/clients.md`; `/get-started/registry.md`; `/rfds/mcp-over-acp.md`.

- What it is: JSON-RPC 2.0 protocol standardizing **editor/IDE (client) ↔ coding agent** interop, LSP-style decoupling; v1 transport is **stdio only** (newline-delimited; stderr for logs); remote Streamable-HTTP/WebSocket transport is an in-progress RFD, not stable (sources: overview.md; transports.md).
- Who speaks it (agents): OpenCode (native `opencode acp`), Cursor (`agent acp` CLI), Codex (via `agentclientprotocol/codex-acp` adapter), Claude (via Zed SDK adapter), Goose, Devin CLI (JetBrains/Zed/Xcode), Pi (via third-party `pi-acp`), plus Gemini CLI, Cline, Copilot CLI (public preview), Junie, and others. Clients: Zed (reference), JetBrains, VS Code extensions, Neovim plugins, and a long tail of desktop/web/mobile/bridge clients (sources: agents.md; clients.md).
- What it carries: sessionJE lifecycle (`initialize`, `authenticate`, `session/new|load|prompt`, `session/cancel`), agent→client `session/update` (message chunks, tool calls, plans, slash-command ads, mode changes, compaction), client→agent `session/request_permission`, filesystem/terminal/elicitation client capabilities, auth methods incl. terminal auth; it **reuses MCP's JSON shapes where possible but is not MCP** (source: overview.md).
- Tool carriage today: the **client injects MCP servers per session** (`session/new` `mcpServers`) — semantics are agent-defined and uneven (OpenCode: stdio+HTTP, headers passthrough, no OAuth, no SSE; Cursor CLI: project/user `mcp.json`, team servers excluded). Draft **MCP-over-ACP RFD** (`type: "acp"`, `serverId`+`requestId` routing, one MCP operation per `mcp/message`, **MCP 2026-07-28 only**, no init handshake, MRTR for interactivity) would let a client supply tools over the ACP channel with no subprocess — explicitly a draft targeting one MCP revision, not a stable install surface (source: mcp-over-acp.md RFD).
- Distribution: the **ACP Registry** (`cdn.agentclientprotocol.com/registry/v1/latest/registry.json`, curated, auth-supporting agents only, PR-based submission) enables one-click install **in supporting ACP clients** (e.g. Athas, Open Knowledge, desktop clients advertising one-click registry install) — i.e. one-click *of agents into editors*, not of site tools into CLI harnesses (source: registry.md).
- Maturity verdict: v1 core stable and broadly adopted via adapters; transports beyond stdio, session/subagent extensions, and MCP-over-ACP are RFD-stage. For this brief, ACP is relevant as (a) how editors absorb whole agents, and (b) the future client-supplied-tool channel — **not** as a present-day install path for a site agent into the seven harnesses.

## 9. Credential custody: who holds what (Q7)

Spec baseline (MCP Authorization, 2025-11-25): authorization is OPTIONAL; HTTP transports SHOULD conform (OAuth 2.1, Protected Resource Metadata RFC 9728, CIMD → pre-reg → DCR priority, `resource` indicators, audience-bound tokens, no passthrough); **stdio transports SHOULD NOT use the spec and instead take credentials from the environment** (source: `https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization`, Purpose/Roles/Client-registration sections).

| Harness | Keyless public MCP | OAuth remote MCP | Static-key remote MCP | Local stdio secrets |
|---|---|---|---|---|
| opencode | just works (`mcp add --url`) | harness holds tokens outside project config (`mcp auth/logout`); CIMD→DCR→pre-reg | `headers: {… "{env:VAR}"}`; never literal in file | `environment:` + `{env:}` |
| Claude Code | just works | `claude mcp login` per endpoint; `remove` deletes stored tokens (revoke via `logout` first); never commit secrets to `.mcp.json` (needs per-clone approval anyway) | `headers` + `headersHelper` (dynamic), `${VAR}` expansion | `env` on stdio entries |
| Cursor | just works | per-machine sign-in; Cloud Agents sign in separately; static `auth.{CLIENT_ID,CLIENT_SECRET,scopes}` + fixed allowlisted callbacks | `headers: {Authorization: Bearer ${env:…}}` | `env`/`envFile` |
| Codex CLI | just works | `codex mcp login`; CIMD→DCR→pre-reg; loopback callback w/ server-specific ID rules | `bearer_token_env_var`, `env_http_headers`, `http_headers_helper` (cached, refreshed on 401/403) | `env`/`env_vars` |
| Pi | `pi mcp add --url` | `pi mcp login` (browser) | extension `env` / settings (user's own files) | extension process env |
| Goose | just works | auto CIMD/DCR; pre-reg `client_id` + `client_secret_key` (reference, value in secret store); `GOOSE_OAUTH_CALLBACK_PORT` | `envs` in config.yaml / `--with-extension "VAR=val cmd…"` | same |
| Devin | just works (HTTP Test-tools) | Connect click; fixed callback `https://api.devin.ai/mcp/oauth/callback`; Access Org (shared service account) vs Personal (per-member) | Auth Header or Secrets `$NAME` in header script (session machine) | env vars in STDIO config (+ Secrets) |

Implications for keyed vs keyless site-agent transports:

1. **Keyless public MCP is the only artifact that is truly one-command everywhere**, including committed project configs (`.mcp.json`, `.cursor/mcp.json`, `.codex/config.toml` project layer, `config.toml`-shipped examples) and deeplinks (`cursor://…`, `goose://…`). No per-user, per-harness credential step; no callback allowlisting (sources: §1–§7 install paths).
2. **Keyed assistant APIs cannot ride committed configs as literals anywhere** — every harness's trust/approval story (Claude per-clone approval, Codex trusted-project-only layer, Pi post-trust project resources, opencode managed-policy precedence) assumes committed files carry no secrets; all seven provide env-indirection instead (table above).
3. **OAuth (per-user) is the least-friction keyed story but is never zero-touch**: each harness needs its own redirect/callback allowlisted at the publisher's authorization server — Cursor needs *two* (`cursor.com/…/callback` + `localhost:8787`), Devin needs its fixed `api.devin.ai` callback, Codex/opencode/Goose need loopback (+ Goose pinned-port var). A publisher shipping OAuth must register all of these up front and document the per-harness Connect click (sources: Cursor static-OAuth section; Devin OAuth-callback note; Codex callback rules; opencode OAuth section; Goose pre-reg section).
4. **Static per-user keys are the headless/CI story** (Codex header-helper refresh, Goose envs, Devin header scripts, Cursor `${env:}`) but push secret distribution onto the user/org (team marketplaces, managed settings, secret stores) — install docs must cover each harness's env plumbing, not just the server URL.
5. Runtime key capture via elicitation is **not** a credential-distribution channel: opencode surfaces MCP elicitation as forms/links but its ACP layer explicitly never sends secret-collecting forms; treat elicitation as untrusted input UX, not key onboarding (sources: opencode mcp-servers Input-requests; cli/acp Questions section).

## 10. Distribution matrix

Columns: **MCP** = connect a site MCP server; **Skill** = ship `SKILL.md`; **Agent** = ship harness agent/subagent definition; **Plugin** = ship harness plugin bundle; **ACP** = harness speaks ACP *as an agent* (drivable from ACP clients); **Mkt** = first-party curated marketplace/install UX. (WebMCP/deeplinks are adjacent mechanisms, §11.)

| Harness | MCP | Skill | Agent | Plugin | ACP | Mkt |
|---|---|---|---|---|---|---|
| opencode V2 | Yes — `mcp add`, `mcp.servers` local/remote, OAuth CIMD→DCR→pre-reg (mcp-servers doc) | Yes — `.opencode/skills/`, compat dirs, HTTP catalogs (skills doc) | Yes — `.opencode/agents/*.md`, `subagent` permission gating (agents doc) | Yes — npm/git/local, `.opencode/plugins/` (plugins doc) | Yes, native — `opencode acp` (cli/acp doc) | No — npm/git/direct + Console workspace push; no curated store (plugins doc; console/harnesses doc) |
| Claude Code | Yes — `claude mcp add`, local/user/project scopes, managed policy (mcp.md; mcp-quickstart.md) | Yes — Agent Skills, 7 locations, rich frontmatter (skills.md) | Yes — `.claude/agents/`, inline `mcpServers`, plugin limits (sub-agents.md) | Yes — `plugin.json`, `.mcp.json`/hooks/agents/skills, `userConfig` secrets (plugins/components.md; plugins/install.md) | Partial — via Zed SDK adapter, not native (ACP agents page) | Yes — marketplaces + Anthropic directory (plugins/install.md) |
| Cursor | Yes — `.cursor/mcp.json`, DCR + static `auth`, team distribution, deeplinks (mcp.md; install-links.md) | Yes — `.agents/skills` + `.cursor/skills`, sync + team publish (skills.md) | Yes — agents bundled in Cursor Plugins; subagent task types (plugins.md; cli/acp.md `cursor/task`) | Yes — Agent + Cursor Plugin formats (plugins.md) | Yes, native — `agent acp` CLI (cli/acp.md) | Yes — Cursor Marketplace (reviewed) + team marketplaces (plugins.md) |
| Codex CLI | Yes — `[mcp_servers]`, CIMD→DCR→pre-reg, header helpers (extend/mcp.md) | Yes — `.agents/skills` walk-up + admin/system, `openai.yaml` tool deps (build-skills.md) | Yes — `.codex/agents/*.toml`, inherits parent perms/MCP (subagents doc) | Yes — universal ChatGPT+Codex plugin directory, skills+MCP+hooks (skills-and-plugins.md; hooks.md) | Partial — via `codex-acp` adapter (ACP agents page) | Yes — shared plugin directory + submit flow (skills-and-plugins.md; build-skills.md) |
| Pi | Yes — extension-registered + `pi mcp add/login` (extensions.md; console/harnesses/pi) | Yes — Agent Skills spec, user/project/`agents/skills` (skills.md) | No — no subagents/plan mode by default; extensions build behavior (README) | Partial — Pi packages ≈ distribution (npm/git/local + gallery) (packages.md) | Partial — via third-party `pi-acp` (ACP agents page; registry page) | Partial — `pi-package` gallery + `pi install`, no curated review stated (packages.md) |
| Goose | Yes — extensions = MCP, UI/CLI/deeplink/config.yaml, auto OAuth (using-extensions.md) | Yes — Agent Skills, `agents/skills` + plugin ns (using-skills.md) | Partial — Summon subagents built-in; custom-agent format not verified here (using-extensions.md; open Q) | Partial — plugins exist as skill/extension source; manifest schema not verified here (using-skills.md; open Q) | Yes — listed ACP agent (ACP agents page; registry page) | Partial — extensions directory + deeplinks; review posture unstated (using-extensions.md) |
| Devin | Yes — Customize→MCPs, marketplace plugins + custom STDIO/SSE/HTTP, Org/Personal access (work-with-devin/mcp.md) | Yes — `SKILL.md` repos/Customize (llms.txt skills entry) | Partial — CLI subagents verified; cloud agent-definition format via plugins/playbooks (llms.txt CLI + playbooks entries) | Yes — personal/org/enterprise scopes + team marketplace (llms.txt plugins entries; mcp.md) | Yes — Devin CLI in ACP editors (cli/acp/zed.md; registry page) | Yes — plugin marketplace + legacy MCP marketplace (mcp.md) |

## 11. Adjacent (non-harness) mechanisms worth knowing

- **WebMCP / Site tools** (ChatGPT desktop built-in browser only): page-registered `document.modelContext.registerTool()` tools, per-invocation safety review, die on navigation; no declarative/iframe support; Enterprise policy-gated. This is the *page-bound* counterpart to MCP: right surface for "same live page + signed-in session" tasks, wrong surface for anything needing persistence or CLI-harness reach (source: `https://developers.openai.com/codex/webmcp/`).
- **Deep links**: Cursor (`cursor://…/mcp/install`) and Goose (`goose://extension?…`) both support URL-encoded one-click MCP installs — the closest thing to an install button outside a marketplace (sources: install-links.md; using-extensions.md Deeplinks).
- **Cross-harness skill compat**: Agent Skills directories are mutually readable across Claude (`.claude/skills`), Cursor (`.cursor/skills`, `.agents/skills`), Codex (`.agents/skills`), Pi (both), Goose (both + `.goose/skills`) — one `SKILL.md` directory can be symlinked/copied across all five (sources: skills pages of each).

## 12. Recommendation: smallest artifact set for one installable row

Ship **two portable artifacts + per-harness thin shims**:

1. **One public, keyless Streamable-HTTP MCP server** exposing a single `ask_<site>` tool (plus a tight server `instructions` string — Codex reads the first ~512 chars for routing). This installs in all seven harnesses with zero credential steps, rides committed project configs, deeplinks, and every marketplace/plugin wrapper. Keep SSE/stdio only as fallback notes (SSE is deprecated in Claude/Cursor/Devin docs).
2. **One portable `SKILL.md`** (`<name>/SKILL.md`; frontmatter limited to the spec intersection: `name`, `description`, optional `license`/`compatibility`/`metadata`; imperative body; relative paths only; no harness-proprietary keys like `openai.yaml` deps, `allowed-tools`, `context: fork`, `paths`-scoping assumptions). It is auto-discovered in all seven harnesses' skill locations and teaches the model when to call the MCP tool and how to phrase questions.
3. **Per-harness install shims** (the irreducible remainder, one short section each): the exact command/config snippet with env-indirection for the key, the OAuth Connect click + callback allowlist entries, the approval/trust click (Claude `.mcp.json`, Codex trusted project, Pi project trust), and the marketplace/plugin listing where one-click matters (Claude marketplace plugin, Cursor Marketplace/team marketplace, Codex plugin directory, Devin Customize plugin, Pi package, Goose directory entry).

What **remains harness-specific no matter what** (do not try to abstract): OAuth client registration and callback sets (§9.3); secret storage and team distribution (managed settings, secret stores, Org-vs-Personal sharing); permission/approval UX and policy hard-denies; tool naming (`<server>_<tool>` vs `mcp__<server>__<tool>` vs `mcp__plugin_*`); agent-definition formats (all five differ); plugin manifests and store review flows; sandboxing postures (Pi: none built-in; Codex: sandbox+approvals; opencode/Claude: permission/policy stacks); and ACP directionality (opencode/Cursor/Devin/Goose speak ACP as agents; nothing here lets a site agent *enter* a CLI harness over ACP — MCP-over-ACP is draft).

If the assistant must be **keyed per user**: prefer OAuth+DCR/CIMD on the public MCP URL (one server, per-user browser sign-in, publisher registers all harness callbacks once) over static keys; reserve static keys + header-helper/secret-store plumbing for headless/CI and shared service accounts (Devin Org access, Cursor team MCP, Codex `bearer_token_env_var`).

## 13. Open questions (primary sources silent or not pulled)

1. Claude `headersHelper` exact schema/refresh semantics and full `${VAR}`-expansion rules (referenced in mcp.md; dedicated schema section not pulled).
2. Cursor `vscode.cursor.mcp.registerServer()` signature and how it handles auth (one-line mention in mcp.md).
3. Goose custom-agent definition format and plugin manifest schema (`Custom Agents`, `Subagents`, `Plugins` guide pages listed in docs nav but not pulled) — matrix marks Partial pending these.
4. Pi `settings.json` MCP-server schema and RPC/SDK auth model (`settings.md`, `rpc.md`, `sdk.md` not pulled; extensions.md covers registration only).
5. Devin plugin manifest/MCP-declaration schema and indexing constraints (plugins guide + indexing-issues pages not pulled).
6. Codex WebMCP reach: docs cover the desktop built-in browser only — whether site tools surface in CLI/IDE/Cloud harnesses is unstated.
7. Cursor Cloud Agents API request/response shapes (endpoints page exists, not pulled); Devin API rate/permission surface for the reverse direction.
8. ACP remote transport and MCP-over-ACP: both RFD-stage; no adoption, conformance, or timeline statement found in primary sources.
9. Team distribution of *keyed* servers via committed configs: client IDs are public but per-harness secret-plumbing docs assume user-held files — exact supported pattern per harness for org-wide keyed rollout (managed `env`, team MCP, secret stores) is fragmented across admin docs not pulled here.
10. OpenCode Console as a *third-party* distribution channel: docs show Console delivering its own providers/MCP to harnesses — whether an outside publisher can distribute their server through a Console workspace (vs. management-plane only) is unstated.
