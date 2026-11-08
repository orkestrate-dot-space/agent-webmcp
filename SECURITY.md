# Security Policy

agent-webmcp ships JavaScript that runs in users' browsers and pulls files
over the network. That makes supply-chain and credential hygiene load-bearing.

## Reporting a vulnerability

Use **GitHub private vulnerability reporting** on this repo
(Security tab → Report a vulnerability). Do not open a public issue for
anything that could leak sessions, credentials, or browser access.

## Scope notes

- Never paste secrets, session tokens, or login credentials into issues,
  specs, evidence files, or chat transcripts. Reports containing them will
  have the values redacted on sight.
- Injected scripts run with page privilege by design. A `sync` source or
  `registry/sites/*.js` entry that exfiltrates data, phones home, or
  touches credentials is in scope and urgent.
- `save` runs untrusted site scripts in a `node:vm` sandbox (5s timeout, no
  `require`/`process`) to extract schemas only. Constructor-chain sandbox
  escape is a known accepted risk — reviewed per release, not per PR.
  Reports demonstrating a practical escape with impact are in scope.
- Out of scope: vulnerabilities in the browser itself, the driver, or the
  harness — report those to their own projects.
