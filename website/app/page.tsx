import SiteNav from "./site-nav";
import { InstallBlock, SetupAgentButton } from "./hero-actions";

const REPO = "https://github.com/system1970/agent-webmcp";

export default function Home() {
  return (
    <>
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:z-50 rounded-md border border-line bg-canvas px-3 py-2 text-sm text-ink no-underline"
      >
        Skip to content
      </a>
      <SiteNav />
      <div className="mx-auto flex min-h-[calc(100svh-4rem)] w-full max-w-[1440px] flex-col px-6">
        <main id="main" className="flex flex-1 flex-col items-center justify-center py-6 text-center">
          <h1 id="claim" className="max-w-[18ch] text-6xl font-medium leading-[1.02] tracking-[-0.04em] max-sm:text-5xl">
            Turn the web into your agent&apos;s toolkit.
          </h1>
          <p className="mt-5 max-w-[52ch] text-xl leading-8 text-ink-2">
            Connect your agent to the tools, assistants, and workflows
            inside websites. Use native website capabilities or turn
            existing browser interactions into custom tools, so your agent
            can work with your software, not just read about it.
          </p>
          <p className="mt-5 text-[13px] text-ink-2">
            <strong className="text-ink">Beta</strong> · <a href={REPO}>v0.1.0-beta.1</a> · Chromium 152
            or newer · Linux-first (Windows, macOS untested)
          </p>
          <div className="mt-7 flex flex-wrap items-start justify-center gap-3">
            <SetupAgentButton />
          </div>
          <div className="mt-7 flex w-full justify-center">
            <InstallBlock />
          </div>
        </main>
        <footer className="flex flex-wrap justify-between gap-2 py-4 text-[13px] text-ink-2">
          <span>
            MIT license · <a href="/llms.txt">llms.txt</a>
          </span>
          <span>© 2026 system1970</span>
        </footer>
      </div>
    </>
  );
}
