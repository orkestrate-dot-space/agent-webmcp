import { defineConfig } from "cf/config";
import * as entrypoint from "./src/index.ts" with { type: "cf-worker" };

export default defineConfig({
	worker: {
		name: "agent-webmcp-index",
		compatibilityDate: "2026-10-01",
		entrypoint,
		assets: { notFoundHandling: "none" },
	},
});
