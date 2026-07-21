// @ts-check
import { defineConfig } from 'astro/config';

// Deployment base path. For a GitHub Pages project site set this to '/snow-cli/'
// (and site to 'https://ewatch.github.io'); internal markdown links are
// rewritten to match via the rehype plugin below.
const SITE_BASE = '/';

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
