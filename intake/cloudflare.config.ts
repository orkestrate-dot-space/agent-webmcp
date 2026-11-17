import { defineConfig, bindings, triggers } from "cf/config";
import * as entrypoint from "./src/index.ts" with { type: "cf-worker" };

export default defineConfig({
	worker: {
		name: "agent-webmcp-intake",
		compatibilityDate: "2026-10-01",
		entrypoint,
		env: {
			// Requests, takedowns and scan state.
			DB: bindings.d1({ id: "f2ef1983-2689-4a36-8cbf-ad6c0c0a6a67" }),
			// The published index: the Worker serves it, this Worker writes it.
			INDEX: bindings.kv({ id: "c489f55fb6d04a0a97a9143c15ec2f33" }),
			// The boxes, comma separated. The first scans submissions; all of
			// them share the weekly rebuild. None holds credentials.
			BOX_IDS: bindings.text(
				"m_agqytbd4onrfszp45jpckqyo24," +
					"m_agqyrnqbsajhc75tb7cajpdquy," +
					"m_agqyta2bbtitdouzbaakhum5si," +
					"m_agqyta2idnn6you6523mnlad3q," +
					"m_agqyta2om5ithdnubribs5cili",
			),
			WORKER_URL: bindings.text("https://agent-webmcp-intake.agent-webmcp.workers.dev"),
			POINTER_URL: bindings.text("https://agent-webmcp-index.agent-webmcp.workers.dev/index.json"),
			// Turnstile secret, the box API key, the admin token and the IP salt
			// are Worker secrets (cf workers secrets), never in this file.
		},
		triggers: [
			// Every minute: start the next queued scan, reap stalled ones.
			triggers.scheduled({ schedule: "* * * * *" }),
			// Monday 06:00 UTC: rebuild the whole index on the box.
			triggers.scheduled({ schedule: "0 6 * * 1" }),
		],
	},
});
