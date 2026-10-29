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
  turbopack: {
    root: dirname(fileURLToPath(import.meta.url)),
  },
};

export default withMDX(nextConfig);
