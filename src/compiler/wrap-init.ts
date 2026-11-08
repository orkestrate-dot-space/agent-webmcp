/** Wrap a raw site body with the flagless polyfill header. Output is a
 * self-contained <domain>.init.js: header first, then the site script. */
export function wrapInit(header: string, body: string, domain: string): string {
  const cleanBody = body.endsWith("\n") ? body : body + "\n";
  return `${header.endsWith("\n") ? header : header + "\n"}\n/* site toolkit: ${domain} (appended by agent-webmcp) */\n;(function(){\n${cleanBody}})();\n`;
}
