export default {
	fetch() {
		return new Response("not found", { status: 404 });
	},
} satisfies ExportedHandler;
