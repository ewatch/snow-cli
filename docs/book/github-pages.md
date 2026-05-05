# Deploying this book to GitHub Pages

This repository includes an mdBook setup for publishing the documentation as a static GitHub Pages site.

## Local preview

Install mdBook:

```bash
cargo install mdbook --locked
```

Serve the book locally:

```bash
mdbook serve
```

Open the local URL printed by mdBook, usually:

```text
http://localhost:3000
```

Build the static site:

```bash
mdbook build
```

The generated HTML is written to:

```text
book/
```

The `book/` directory is generated output and should not be committed.

## GitHub Pages setup

The workflow in `.github/workflows/pages.yml` builds and deploys the book with GitHub Actions.

In GitHub, configure Pages like this:

```text
Repository → Settings → Pages → Build and deployment → Source → GitHub Actions
```

Then push changes to `main`, or run the workflow manually from the Actions tab.

## Taking the page down

To unpublish the site quickly:

1. Go to `Repository → Settings → Pages`.
2. Disable Pages or change the source away from the Pages workflow.
3. Optionally disable or delete `.github/workflows/pages.yml`.

Making the repository private usually removes public access shortly after GitHub updates Pages visibility, but disabling Pages is the most explicit option.

## Project pages URL

For a repository named `snow-cli`, GitHub Pages typically publishes to:

```text
https://<owner>.github.io/snow-cli/
```

If the repository name changes, update `site-url` in `book.toml`.
