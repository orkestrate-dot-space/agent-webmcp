# usage

The directory's usage counter: a Cloudflare Worker in front of D1.
`POST /use` increments one UTC-hour bucket per domain; `GET /use`
returns a zero-filled series for the chart.

This is deliberately **not** `agent-webmcp-sites` (the proof ledger).
That worker is deployed and owns its own routes; this one is a separate
service on a separate D1 database so redeploying a counter can never
touch the ledger.

## Contract

```
POST /use            { "domain": "mintlify.com" }        -> 204
GET  /use?domain=…&days=1|7|30|90                        -> { domain, total, days }
GET  /health                                              -> { ok: true }
```

- `days=1` returns 24 hourly buckets (`HH:00`); anything else returns
  daily buckets (`MM/DD`), 1–90 days.
- One row per domain per UTC hour (`usage.domain`, `usage.bucket`,
  `usage.count`). Daily views aggregate in the worker.
- No identity, no IP, no query text, no cookies. Only domain + hour +
  count.
- `POST` is capped at 50,000/day per domain (`429` with `Retry-After`).
  The chart is a public directional metric, not an audited ledger.

## Deploy

```sh
bun install
bunx wrangler d1 create agent-webmcp-usage   # first time; copy the id
# put database_id into wrangler.jsonc
bunx wrangler d1 execute agent-webmcp-usage --remote --file=./schema.sql
bunx wrangler deploy
```

Wrangler needs `CLOUDFLARE_API_TOKEN` + `CLOUDFLARE_ACCOUNT_ID` in the
environment (the `cf` CLI's OAuth token works).

## Site wiring

`website/app/api/use/route.ts` proxies here. The default URL is

```
https://agent-webmcp-usage.agent-webmcp.workers.dev
```

Override with `USAGE_API_URL` if the worker moves. No token: the
worker is write-only from the site's perspective and validates the
domain; the proxy keeps the browser same-origin and hides the worker
URL. If the worker is unreachable, the route falls back to the local
`data/agent-usage.jsonl` for offline `next dev` only.

## Free-tier math

| Resource | Free allowance | This worker |
| --- | --- | --- |
| Workers requests | 100,000/day | 1 per beacon + 1 per chart read |
| D1 rows written | 100,000/day | 1 per beacon (hourly upsert) |
| D1 rows read | 5,000,000/day | ≤ 24 per cap check + ≤ 2,160 per 90-day chart |
| D1 storage | 5 GB | ~2160 rows per domain per 90 days |

A launch at thousands of beacons/day is two orders of magnitude under
the ceiling. Past 100,000 requests/day, Workers Paid is $5/month and
removes the request cap; D1 Paid includes 50M writes/month.

Schema: `schema.sql`.
