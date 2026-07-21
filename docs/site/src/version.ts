// Injected at build time by Vite `define` (see astro.config.mjs), which reads
// the crate version from the repo-root Cargo.toml. Declared here so this module
// type-checks; the token is replaced textually during the build.
declare const __SNOW_CLI_VERSION__: string;

/**
 * Displayed snow-cli version, e.g. `0.7.0`, sourced from the repo-root
 * Cargo.toml at build time. Prefix with `v` where a tag-style label is wanted.
 * Bumping Cargo.toml (which every release does) re-bakes the docs on the next
 * Pages deploy, so the badge and footer stay in lockstep with releases.
 */
export const VERSION = __SNOW_CLI_VERSION__;
