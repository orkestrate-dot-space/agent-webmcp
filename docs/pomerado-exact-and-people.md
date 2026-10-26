# Pomerado.ai — Exact Identity + People in the Same Space

**Date:** 2026-10-26

---

## Part 1 — What Pomerado exactly is

**Pomerado** (Pomerado AI, Inc., YC Fall 2026, 2 people, San Francisco) is a **web-to-agent tool platform**: it turns *any* website (including ones with no API, behind logins/2FA, legacy portals, internal tools) into a **deterministic, hosted, self-healing API/MCP integration** that an AI agent can call with typed inputs/outputs instead of driving a browser.

### Mechanism (open-source core, MIT, 583★)
- A **minter** = a model agent with a real Chromium browser (Playwright) + an LLM. It explores the live site, writes operation modules under `src/`, runs them, and publishes an entrypoint with JSON Schemas for input/output.
- **Guardian** reviews every browser action and the finished source while minting.
- **Runtime** serves each integration as its own MCP server (one tool per published tool) on your machine or in the cloud.
- **Job model**: `mint`/`get_job`/`provide_input`/`cancel_job`, 30s polling, 20-min active-work budget, questions routed to you.
- **Authority**: `read` vs `write` chosen at build time; sign-in origin allow-listing; PII/PHI stripping; provider-credential screening.

### Cloud additions (managed)
Hosting, OAuth/saved logins (with 2FA via authenticator/recovery codes or a Pomerado-owned phone number), access control, managed browser fleet (anti-bot), breakage detection + auto-repair, REST API (`api.pomerado.ai/v1`), dashboard, webhooks, teams/roles/audit (Business).

### Pricing
Free ($0, 100 calls/day, 10 integrations/mo) · Premium ($50/mo, 1,000 calls/day, 100/mo) · Business (contact, 100k+ calls/day, unlimited accounts/site, private integrations, SLAs).

### Positioning vs. browser agents
Browser agents reason through every page (slow, expensive, non-deterministic). Pomerado mints a deterministic API once, then runs it in seconds. Claimed: 7× faster, 100× cheaper, 30% more accurate than Astra/Instinct/Muse computer use on a 30-task/25-business-app benchmark; customer case: onboarding 2 employees across HR/IT/courses/library in 8m16s for $0.03 vs Codex Computer Use 21m22s for $41.01.

### Founders
- **Akshay Manglik** — Co-founder & CEO (ex-Scale AI).
- **Alan Ma** — Co-founder & CTO.

### Exact category
**"Web → Agent Tool" / minted deterministic API-MCP from any site (no site cooperation).** This is a sub-category of the broader "make websites agent-callable" space, distinct from (a) browser/computer-use agents, (b) web scraping/extraction APIs, (c) iPaaS/unified APIs (existing APIs only), (d) RPA, and (e) the emerging site-cooperative **WebMCP** standard.

---

## Part 2 — All the people in the same space

Grouped by mechanism. Founders + affiliation + batch.

### Tier A — Direct: minted deterministic API/MCP from any site (no site cooperation)

| Company | Founders | Batch / Stage | Notes |
|---|---|---|---|
| **Pomerado** | Akshay Manglik (CEO), Alan Ma (CTO) | YC F26, 2ppl, SF | Minted API/MCP, self-heal, auth brokerage, `read`/`write` authority. MIT core. |
| **Rindler** | Michael Serrano (CEO), Arthur De Los Santos (CTO) | YC S26, 2ppl, Boston (MIT '26) | Hosted MCP, deterministic typed tools, logins/2FA/bot detection, "IDE for tokenmaxxers". Agent-agnostic. |
| **Parse** (parse.bot) | Alex Forman (solo) | YC F25, 3-6ppl | Reverse-engineers network requests → deterministic API, 10-100× faster/cheaper than browser automation. Added MCP Mar 2026. Dropped out of Berkeley. |
| **PulpMiner** | Melvin George (solo) | Indie (AppSumo) | Turn webpage → real-time JSON API; dynamic URL vars, scraper selection. |
| **Context.dev** | Yahia Bakour (solo) | YC S26, 4-6ppl, SF; $9M seed | Structured web data + brand/styleguide + monitors + MCP. More extraction than task execution, but adjacent; ex-Sunrun/Amazon, 2 prior exits. |
| **Kimono Labs** (historical) | Ryan Rowe, Pratap Ranade | YC 2014, acquired by Palantir 2016 | "Turn websites into structured APIs from browser." Shut down post-acquisition. The original precedent. |

### Tier B — "Build APIs for any website" but extraction/data-focused (adjacent)

| Company | Founders | Batch | Notes |
|---|---|---|---|
| **Reworkd** | Asim Shrestha (CEO), Adam Watkins (CTO), Srijan Subedi (COO) | YC S23, Vancouver/SF | End-to-end web data pipeline; Playwright code gen; $4M raised (Paul Graham, Nat Friedman, Daniel Gross). |
| **Firecrawl** | Caleb Peffer, Eric Ciarla, Nicolas Silberstein Camara | YC S22; $75M Series B | Crawl/scrape/extract/interact + MCP; 1.5M+ users. Not minted-API but extraction+interaction. |
| **Asteroid** | David Mlcoch, Joe Hewett | YC W25, London | Self-healing browser agents (healthcare/insurance/finance); Astro no-code builder. |
| **Bytebot** | Carl Atupem | YC S21 (acquired) | Open-source container for desktop/browser agents. |

### Tier C — Browser automation exposed as MCP tools (universal fallback)

| Company | Founders | Notes |
|---|---|---|
| **Browser Use** | Gregor Zunic (CTO), Magnus Müller | Open source (78k-119k★) + cloud; WebMCP directory scanner (13,352 sites). |
| **Skyvern** | (YC-backed) | Vision-first, write-heavy behind logins; MCP server. |
| **Stagehand / Browserbase** | Paul Klein (CEO) + team | Playwright + AI escape hatches; managed cloud browsers. |
| **Steel** | open source | Headless browser API + proxies + anti-detection. |
| **BrowserOS** | Nikhil Sonti, Nithin Venkat Sonti (twins, ex-Google/Meta) | YC S24. Open-source agentic browser. |
| **Hyperbrowser** | (YC) | Web infra for agents: stealth, proxies, CAPTCHA, sessions. |

### Tier D — WebMCP (site-cooperative standard, first-party tool exposure)

| Actor | Role | Notes |
|---|---|---|
| **Google** (Chrome team, Khushal Sagar et al.) | Co-author | Origin trial Chrome 149–156; security spec still placeholder. |
| **Microsoft** (Edge team) | Co-author | W3C WebML Community Group. |
| **MCP-B** | Alex Nahas (Amazon, ex-) | Polyfill + `@mcp-b/webmcp-local-relay` bridging WebMCP tools into Claude Desktop. |
| **Webfuse, vensas** | Widget + token connect UX | Site-side connect widgets. |
| **Browser Use** | Scanner | 13,352 sites / 45,763 tools indexed (Oct 19, 2026). |

### Tier E — Integration / iPaaS MCPs (existing APIs only, adjacent)

Composio (open source, 27k★) · Pipedream (acquired by Workday, Nov 2025) · Merge · Nango · StackOne · Arcade AI · Paragon · Klavis · Superface ("self-healing integrations") · Glama · Manufact (mcp-use, YC) · Orthogonal (YC W26, agentic payments for APIs) · Klavis · Obot (ACORN Labs).

### Tier F — Extraction / web-data MCPs (read-only content)

Firecrawl · Apify · Bright Data · Tavily · Exa · Jina Reader · ScrapingBee · ZenRows · Scrapfly · Oxylabs · Crawl4AI · ScrapeGraphAI · Crawlee · String · AlterLab · CrawlForge · Olostep · Context.dev (also Tier A) · Parse (also Tier A).

---

## Part 3 — The closest "head-to-head" trio

The three companies doing the **exact same thing** (mint a deterministic API/MCP from any site without site cooperation) are all YC, all 2025–2026:

1. **Parse** (Alex Forman, YC F25) — reverse-engineers *network requests* → deterministic API; 10-100× faster/cheaper than browser automation; added MCP Mar 2026; 30k+ users in 3 weeks.
2. **Rindler** (Michael Serrano + Arthur De Los Santos, YC S26, MIT '26) — hosted MCP, deterministic typed tools, logins/2FA/bot detection, self-heal-on-change.
3. **Pomerado** (Akshay Manglik + Alan Ma, YC F26) — LLM minter ("Guardian") + self-heal + auth brokerage + `read`/`write` authority + hosted.

**Differentiation:** Parse bets on reverse-engineering network calls (fastest/cheapest, but breaks on SPAs/complex flows); Rindler bets on pre-onboarded sites + deterministic typed tools; Pomerado bets on LLM-driven minting + self-healing + auth brokerage + agent-driven build loop. All three are 2-6 person teams competing for the same "any website → agent tool" market against browser agents (Browser Use, Skyvern, Stagehand) and the emerging WebMCP standard.

---

## Sources
- https://pomerado.ai/ · https://pomerado.ai/about · https://docs.pomerado.ai · https://github.com/Pomerado/pomerado
- https://www.ycombinator.com/companies/pomerado · https://www.ycombinator.com/companies/rindler · https://www.ycombinator.com/companies/contextdev · https://www.ycombinator.com/companies/parse-bot
- https://www.producthunt.com/products/pomerado/alternatives
- https://github.com/api-evangelist/rindler · https://parse.bot · https://www.pulpminer.com · https://www.context.dev
- https://developer.chrome.com/docs/ai/webmcp · https://webmachinelearning.github.io/webmcp · https://browser-use.com/webmcp
- https://www.stackone.com/blog/ai-agent-tools-landscape-2026
- TechCrunch (Reworkd, Kimono Labs), LinkedIn, Founders Inc, Startup Intros, Crunchbase/Netrows, ScrapeOps, VentureBeat, InfoQ, Nearform, DataCamp, Locomotive, codercops, suganthan, vensas, Moz
