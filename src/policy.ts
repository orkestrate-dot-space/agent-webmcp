// Policy: one flag, no files. --consequential allow|ask|deny,
// default ask. Garbage fails loud at startup (usage error, exit 2).
export type Policy = "allow" | "ask" | "deny";

export const DEFAULT_POLICY: Policy = "ask";

export function parsePolicy(raw: string | undefined): Policy {
  if (raw === undefined || raw === "") return DEFAULT_POLICY;
  if (raw === "allow" || raw === "ask" || raw === "deny") return raw;
  throw new Error(`bad --consequential '${raw}': want allow|ask|deny`);
}

// Verb-level gate. ask without confirm fails the CALL as data (the agent
// sees the fix in-band and resends); deny always fails the call.
export function consequentialGate(policy: Policy, confirmed: boolean, toolName: string, isConsequential: boolean): string | null {
  if (!isConsequential || policy === "allow") return null;
  if (policy === "deny") return `${toolName}: blocked by --consequential=deny.`;
  return confirmed
    ? null
    : `${toolName}: consequential tool needs confirm:true — resend with confirm, or serve with --consequential=allow.`;
}
