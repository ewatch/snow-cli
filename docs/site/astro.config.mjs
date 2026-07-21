// @ts-check
import { defineConfig } from 'astro/config';

// Deployment base path. GitHub Pages serves this project repo at
// https://ewatch.github.io/snow-cli/, so production builds must use the
// '/snow-cli/' base — set via BASE_PATH in .github/workflows/pages.yml.
// Local `astro dev`/`astro build` default to '/' so the landing page is served
// at the root during development. Internal markdown links are rewritten to
// match the base via the rehype plugin below.
const SITE_BASE = process.env.BASE_PATH || '/';

function rehypeBaseLinks() {
  const prefix = SITE_BASE.replace(/\/$/, '');
  /** @param {any} node */
  const walk = (node) => {
    if (
      node.type === 'element' &&
      node.tagName === 'a' &&
      typeof node.properties?.href === 'string' &&
      node.properties.href.startsWith('/')
    ) {
      node.properties.href = prefix + node.properties.href;
    }
    for (const child of node.children ?? []) walk(child);
  };
  return (/** @type {any} */ tree) => {
    if (prefix) walk(tree);
  };
}

export default defineConfig({
  site: 'https://ewatch.github.io',
  base: SITE_BASE,
  markdown: {
    shikiConfig: {
      theme: 'everforest-dark',
    },
    rehypePlugins: [rehypeBaseLinks],
  },
});
