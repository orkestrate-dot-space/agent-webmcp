# Intake

Submissions, takedowns, and the weekly rebuild for the agent-webmcp index.

```
site form  ──POST /submit──▶  intake Worker  ──exec──▶  instabox box
                                   │                      (scanner user)
                                   │◀──POST /callback─────┘
                                   ▼
                             KV: version, pointer  ──▶  agent-webmcp-index Worker
```

## What lives where

| Piece | Where |
|---|---|
| Requests, takedowns, rebuilds | D1 `agent-webmcp-intake` (schema in `schema.sql`) |
| The live index, takedown list, seed list | KV `agent-webmcp-index` (`pointer`, `version:*`, `denylist`, `seeds`) |
| Scans | instabox box `box-spare`, as user `scanner` (no credentials) |
| Box scripts | `/opt/awm-scan/{run-scan,scan-job,rebuild-job}.sh`, root-owned, read-only |

The pointer is the only mutable index key; a version is written once, so a
client never reads a half update. KV takes up to about a minute to propagate,
so a just-published listing can take that long to appear everywhere.

## Secrets

| Name | What |
|---|---|
| `INSTABOX_KEY` | instabox API key, scopes `machines:read,machines:write` (org-wide: it is the only place it exists) |
| `ADMIN_TOKEN` | bearer for `/admin/*` and for skipping Turnstile in a maintainer re-check |
| `IP_SALT` | salt for hashing submitter addresses |
| `TURNSTILE_SECRET` | Turnstile widget secret (empty disables the check: local dev only) |

```console
cd intake
cf workers secrets update INSTABOX_KEY --worker agent-webmcp-intake --type secret_text --text "$KEY"
cf deploy
```

## Routes

| Route | Who |
|---|---|
| `POST /submit` `{url, pages?, turnstile?}` | the site form |
| `GET /status?id=` | the status page |
| `POST /takedown` `{host, turnstile?}` | the site form |
| `GET /seeds.txt` | the rebuild job |
| `POST /callback?id=&token=` | the scan job (one-time token) |
| `POST /callback/rebuild?token=` | the rebuild job |
| `GET /admin/queue` | maintainer |
| `POST /admin/pump` | maintainer: run the queue now |
| `POST /admin/retry?id=` | maintainer: requeue a finished request |
| `POST /admin/rebuild` | maintainer: start the weekly rebuild now |

Cron: `* * * * *` pumps the queue (one scan at a time), `0 6 * * 1` starts the
weekly rebuild.

## Listing rules

A result is listed automatically only when all hold:

- the scan verified at least one tool;
- every tool has a description and no description is flagged;
- the record's host matches the submission;
- the host is not on the takedown list.

Anything else gets its status page and no listing. A browser without the WebMCP
page API reports `error`, never `no_tools`, so a broken scanner cannot delist a
site.

## Weekly rebuild

The sandbox scans `seeds.txt` (published with every index), merges the result,
and posts it back; the Worker publishes it unless verified sites collapse
(below 90% of the current count, with a floor of 20). A refused rebuild keeps
the old index and records why in D1.

## Box setup (once per box)

```bash
sudo useradd -m -s /bin/bash scanner
sudo mkdir -p /opt/awm-scan /var/lib/awm-scan/jobs /var/lib/awm-scan/rebuild
# tools/ comes from this repo
sudo cp -r tools /opt/awm-scan/
sudo chown -R root:root /opt/awm-scan && sudo chmod -R a+rX /opt/awm-scan
sudo cp intake/box/*.sh /opt/awm-scan/ && sudo chmod 755 /opt/awm-scan/*.sh
sudo chown -R scanner:scanner /var/lib/awm-scan
```

Refresh `tools/` after changing the scanner; the box never holds a token.

## Rollback

```console
node tools/index-rollback.mjs            # list versions
node tools/index-rollback.mjs index-<sha>.json
node tools/publish-index.mjs --adopt https://agent-webmcp-index.agent-webmcp.workers.dev/index.json
```

`publish-index --kv` refuses to drop live sites unless `--force`: the local
`index/index.json` is a snapshot, and the bot adds sites between edits.
