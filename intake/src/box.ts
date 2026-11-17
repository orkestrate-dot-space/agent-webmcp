/// Driving the scan boxes through the instabox HTTP API.
///
/// A scan is started detached and reports back to /callback; nothing waits on
/// the scan inside a Worker invocation. The API key lives only here.

import type { Env } from "./env";

const API = "https://api.instabox.sh";

export interface ExecResult {
  stdout: string;
  stderr: string;
  exitCode: number;
  timedOut: boolean;
}

export function boxes(env: Env): string[] {
  return env.BOX_IDS.split(",").map((b) => b.trim()).filter(Boolean);
}

export async function exec(env: Env, boxId: string, cmd: string, timeout = 30): Promise<ExecResult> {
  const res = await fetch(`${API}/v1/machines/${boxId}/exec`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${env.INSTABOX_KEY}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ cmd, timeout }),
  });
  if (!res.ok) {
    const text = (await res.text()).slice(0, 300);
    throw new Error(`box exec failed: HTTP ${res.status} ${text}`);
  }
  return (await res.json()) as ExecResult;
}

/// Start the scan detached: the exec returns in about a second, and the job
/// posts its result to /callback when it is done.
export async function startScan(
  env: Env,
  opts: { id: string; entry: string; extras?: string[]; callback: string; token: string },
): Promise<void> {
  const cmd = [
    "sudo -u scanner",
    "nohup",
    "/opt/awm-scan/scan-job.sh",
    shellQuote(opts.id),
    shellQuote(opts.entry),
    shellQuote(opts.callback),
    shellQuote(opts.token),
    shellQuote((opts.extras ?? []).join("\n")),
    ">/dev/null 2>&1 &",
  ].join(" ");
  await exec(env, boxes(env)[0], cmd, 30);
}

/// Start one shard of the weekly rebuild on one box; it posts its partial
/// index back and the Worker merges when every shard has reported.
export async function startRebuildShard(
  env: Env,
  opts: { id: string; callback: string; token: string; boxId: string; shard: number; count: number },
): Promise<void> {
  const cmd = [
    "sudo -u scanner",
    "nohup",
    "/opt/awm-scan/rebuild-job.sh",
    shellQuote(opts.id),
    shellQuote(opts.callback),
    shellQuote(opts.token),
    shellQuote(env.WORKER_URL),
    shellQuote(env.POINTER_URL),
    shellQuote(String(opts.shard)),
    shellQuote(String(opts.count)),
    ">/dev/null 2>&1 &",
  ].join(" ");
  await exec(env, opts.boxId, cmd, 30);
}

/// A POSIX single-quoted argument. Hosts and ids are validated before they get
/// here, so this is belt and braces.
export function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}
