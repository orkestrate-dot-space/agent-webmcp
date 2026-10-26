# Security Policy

agent-webmcp attaches tabs in reachable browsers and injects page verbs.
That makes credential hygiene load-bearing.

## Reporting a vulnerability

Use **GitHub private vulnerability reporting** on this repo
(Security tab → Report a vulnerability). Do not open a public issue for
anything that could leak sessions, credentials, or browser access.

## Scope notes

- Never paste secrets, session tokens, or login credentials into issues,
  specs, evidence files, or chat transcripts. Reports containing them will
  have the values redacted on sight.
- The engine never stores credentials: auth lives in the browser profile
  or the driver's vault. A report showing otherwise is in scope and urgent.
- Out of scope: vulnerabilities in the browser itself, the driver, or the
  harness — report those to their own projects.
