# snow-cli documentation site

Astro-based documentation for [snow-cli](https://github.com/ewatch/snow-cli), styled after the
project's landing page (dark terminal theme, mint signal color, mono accents). Content originates
from the mdbook sources in `servicenow-cli/docs/book`.

## Commands

```bash
npm install
npm run dev       # local dev server at http://localhost:4321
npm run build     # static build into dist/
npm run preview   # serve the production build locally
```

## Structure

- `public/index.html` — the static landing page (self-contained HTML, served at `/`; screenshots in `public/assets/`)
- `src/content/docs/` — the markdown chapters (copied from the mdbook; internal links rewritten to site routes; the introduction chapter is served at `/introduction/`)
- `src/navigation.ts` — sidebar groups and reading order (mirrors the mdbook `SUMMARY.md`); prev/next pagination is derived from it
- `src/layouts/DocsLayout.astro` — nav, sidebar, on-this-page rail, footer, and all styling
- `src/pages/[...slug].astro` — catch-all route rendering the docs collection

## Adding a page

1. Add the markdown file under `src/content/docs/` (no frontmatter needed; the first `# H1` becomes the page title).
2. Add it to `src/navigation.ts` so it appears in the sidebar and pagination.

## Deploying to GitHub Pages (project site)

In `astro.config.mjs` set `SITE_BASE` to `'/snow-cli/'`. Markdown links are root-relative and are
prefixed with the base automatically at build time; layout links use `import.meta.env.BASE_URL`.
