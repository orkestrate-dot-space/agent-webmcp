# Contributing a site

To list a site in the agent-webmcp index, open an **Add a site** issue and fill
in the form. No code, no ownership proof, no account: the bot lists only what
your pages register with WebMCP on.

## What happens

1. A bot loads your pages in a fresh Chromium (with the WebMCP feature flag)
   and records the tools they register. It reads only: no tool is invoked, no
   form is submitted, no bot check is solved.
2. Tools found and the result is clean → the bot opens a listing pull request.
   A maintainer merges it, and the next index publish lists you.
3. No tools, a bot check, or a load failure → the bot says so on the issue and
   adds `needs-review`. A maintainer confirms and closes it.

Extra pages (up to 3) are checked alongside the homepage. Only `https` pages
on public addresses are loaded.

## What gets listed

- Tools registered at page load through the WebMCP page API.
- Each tool needs a name, a plain description of what it does, and a
  complete input schema. The bot's comment lists anything to fix; fixing it
  ranks the site higher in `find`.

## What is not indexed yet

- Tools that appear only after logging in, searching, or otherwise
  interacting with the page.
- Sites behind a bot check or login wall.

## Staying listed

The whole index is rechecked weekly. A site that shows no tools keeps its
listing through one missed check (marked with the date its tools were last
seen) and drops on the second. Fix the site and file again; the bot rechecks
from scratch.

## Takedown

File a takedown issue (or ask on the listing PR) and the host goes on the
denylist: never scanned, never listed, no questions asked.
