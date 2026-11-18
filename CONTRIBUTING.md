# Contributing a site

To list a site in the agent-webmcp index, open **Add your site** on
[the sites page](https://web.orkestrate.space/webmcp/sites) and paste the URL.
No account, no code, no ownership proof: the bot lists only what your pages
register with WebMCP.

## What happens

1. The bot loads your pages in a fresh Chromium (Chrome 157 with the WebMCP
   feature flag) and records the tools they register. It reads only: no tool is
   invoked, no form is submitted, no bot check is solved.
2. A clean result is listed automatically, within a minute or two. The page you
   came from shows the verdict, and a link shows the check whenever you like.
   A site is listed when the scan found at least one tool, every tool has a
   description, no description is flagged, the host matches what you submitted,
   and the host is not on the takedown list.
3. Anything else is reported on the status page and not listed. You can fix the
   site and submit again.

Extra pages (up to 3) are checked alongside the homepage. Only `https` pages on
public addresses are loaded.

## What gets listed

- Tools registered at page load through the WebMCP page API.
- Each tool needs a name, a plain description of what it does, and a complete
  input schema. The status page lists anything to fix; fixing it ranks the site
  higher in `find`.

## What is not indexed yet

- Tools that appear only after logging in, searching, or otherwise interacting
  with the page.
- Sites behind a bot check or login wall.

## Staying listed

The whole index is rechecked weekly. A site that shows no tools keeps its
listing through one missed check (marked with the date its tools were last
seen) and drops on the second. Fix the site and submit again; the bot rechecks
from scratch.

## Takedown

Email support@orkestrate.space from an address at the domain, or with another
way to show you run it. A maintainer puts the host on the takedown list: it is
removed from the index, never scanned again, and new submissions for it are
refused.
