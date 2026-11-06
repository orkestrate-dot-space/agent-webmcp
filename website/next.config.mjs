import { dirname } from 'path';
import { fileURLToPath } from 'url';
import nextMDX from '@next/mdx';

const withMDX = nextMDX({
  extension: /\.mdx?$/,
  options: {
    // String-form plugin names (serializable — Turbopack/webpack safe).
    remarkPlugins: ["remark-gfm"],
    // Plugin NAMES, not imports: Turbopack's loader options must be
    // serializable, and functions are not. Bakes heading ids at build
    // (github-slugger). The TOC reads them from the DOM; the search
    // indexer must slug identically — see AGENTS.md.
    rehypePlugins: ['rehype-slug'],
  },
});

/** @type {import('next').NextConfig} */
const nextConfig = {
  pageExtensions: ['ts', 'tsx', 'mdx'],
  // Headless-Chrome hydration: the dev server self-identifies as
  // `localhost`, so HMR WebSocket upgrades from `127.0.0.1` origins are
  // refused and the App Router never hydrates headless (SSR shell only,
  // no fibers, registerTools effects never run). Allow the loopback origin
  // and decouple hydration from the React debug channel close-chunk,
  // which never arrives over the headless HMR socket.
  allowedDevOrigins: ['127.0.0.1'],
  experimental: {
    reactDebugChannel: false,
  },
  turbopack: {
    root: dirname(fileURLToPath(import.meta.url)),
  },
};

export default withMDX(nextConfig);
