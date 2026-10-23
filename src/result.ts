// Result kit: typed errors without a framework. Every fallible core
// function returns Result or throws EngineFailed. No exceptions cross
// the verb boundary as-is — verbs translate them to {NO_SUCH_TOOL…}
// reasons plus a human fix.
export type Result<T, E = EngineFailed> = { ok: true; v: T } | { ok: false; e: E };

export const ok = <T>(v: T): Result<T, never> => ({ ok: true, v });
export const err = <E>(e: E): Result<never, E> => ({ ok: false, e });

export class EngineFailed extends Error {
  readonly reason: string;
  readonly fix: string;
  constructor(reason: string, message: string, fix: string) {
    super(message);
    this.reason = reason;
    this.fix = fix;
  }
}

export const fail = (reason: string, message: string, fix: string): EngineFailed =>
  new EngineFailed(reason, message, fix);

// Boundary wrapper: unknown throws (JSON, CDP, disk) become EngineFailed
// with the operation named, so strangers see what broke where.
export async function tryOp<T>(operation: string, fix: string, fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (e) {
    if (e instanceof EngineFailed) throw e;
    throw fail("io", `${operation}: ${e instanceof Error ? e.message : String(e)}`.slice(0, 200), fix);
  }
}
