import { defineConfig, bindings } from "cf/config";
import * as entrypoint from "./src/index.ts" with { type: "cf-worker" };

export default defineConfig({
	worker: {
		name: "agent-webmcp-index",
		compatibilityDate: "2026-10-01",
		entrypoint,
		assets: { notFoundHandling: "none", runWorkerFirst: true },
		env: {
			// The published index lives in KV so the intake bot can update it
			// without redeploying this Worker. Static assets stay as fallback.
			INDEX: bindings.kv({ id: "c489f55fb6d04a0a97a9143c15ec2f33" }),
		},
	},
});
