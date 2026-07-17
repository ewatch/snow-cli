#!/usr/bin/env node
// Headless-browser SN-Utils bridge harness for scripts/e2e-run's
// `sn-utils-bridge` scenarios. Opt-in only: scripts/e2e-run spawns this
// process (and npm-installs this directory) only when SNOW_E2E_SN_UTILS=1.
//
// What it does:
//   1. Downloads the real SN-Utils extension from the Chrome Web Store
//      (same mechanism Chrome itself uses to update extensions) and unpacks
//      it into the caller's scratch dir.
//   2. Patches the local unpacked copy ONLY (never redistributed) so its
//      helper tab can point at an isolated bridge port instead of the
//      hardcoded 127.0.0.1:1978 — running against the real default port
//      would evict a real, already-connected daily-driver ScriptSync tab.
//      Both patches match on exact literal strings found in the extension
//      as of 2026-07; if SN-Utils changes them upstream, this fails loudly
//      instead of silently patching nothing (which would silently reintroduce
//      the collision risk).
//   3. Launches headless Chromium with the patched extension loaded, using
//      the two flag workarounds validated by hand (see README below).
//   4. Logs into SNOW_E2E_INSTANCE_URL with SNOW_E2E_USERNAME/PASSWORD via
//      ServiceNow's standard basic-auth login form.
//   5. Opens the extension's ScriptSync helper tab pointed at the isolated
//      port. This alone is enough to make `snow-cli snu broker status`
//      report browser_connected: true.
//   6. Attempts to capture a live session via SN-Utils' in-page `/token`
//      command. THIS STEP IS NOT IMPLEMENTED: the exact trigger UI for
//      `/token` inside a logged-in ServiceNow tab was not discoverable from
//      the extension's source without a live instance to observe it against
//      (see tests/e2e/README.md). Rather than guess at DOM interactions
//      against a real ServiceNow instance — which could misfire — this
//      harness reports `token_capture: "not_implemented"` in its ready
//      signal and leaves session-dependent scenarios to fail cleanly on
//      their own `/token`-wait timeout, same as an unattended broker today.
//   7. Writes a ready-signal JSON file and blocks until SIGTERM/SIGINT, at
//      which point it closes the browser and exits.

const { chromium } = require("playwright");
const fs = require("fs");
const path = require("path");
const os = require("os");
const { spawnSync } = require("child_process");

const EXTENSION_ID = "jgaodbdddndbaijmcljdbglhpdhnjobg"; // "SN Utils - Tools for ServiceNow"

function log(msg) {
  console.error(`[snu-harness] ${msg}`);
}

function fail(msg) {
  log(`FATAL: ${msg}`);
  process.exit(1);
}

function requireEnv(name) {
  const value = process.env[name];
  if (!value) fail(`missing required env var ${name}`);
  return value;
}

async function downloadExtension(scratchDir) {
  const crxPath = path.join(scratchDir, "sn-utils.crx");
  const extDir = path.join(scratchDir, "sn-utils-ext");
  fs.mkdirSync(extDir, { recursive: true });

  const url =
    "https://clients2.google.com/service/update2/crx" +
    "?response=redirect&acceptformat=crx2,crx3&prodversion=120.0" +
    `&x=id%3D${EXTENSION_ID}%26installsource%3Dondemand%26uc`;
  log(`downloading SN-Utils extension (id=${EXTENSION_ID})`);
  const res = await fetch(url);
  if (!res.ok) {
    fail(`extension download failed: HTTP ${res.status}`);
  }
  const buf = Buffer.from(await res.arrayBuffer());
  fs.writeFileSync(crxPath, buf);

  // CRX = a small header + a standard zip payload; `unzip` tolerates the
  // leading header bytes by scanning for the end-of-central-directory record
  // from the end of the file, so no CRX-specific parsing is needed here.
  const result = spawnSync("unzip", ["-o", "-q", crxPath, "-d", extDir]);
  if (result.status !== 0 && result.status !== 1) {
    // unzip exits 1 for the harmless "extra bytes before zipfile" warning
    // that every CRX produces; only treat other exit codes as fatal.
    fail(`unzip of downloaded extension failed: ${result.stderr}`);
  }
  return extDir;
}

/// Patches the local unpacked extension copy so its helper tab can be told
/// (via a `?port=` query param) to connect the bridge WebSocket to an
/// isolated port instead of the hardcoded 1978. Exits fatally if either
/// expected literal isn't found, rather than silently leaving the extension
/// unpatched and reintroducing the port-collision risk this harness exists
/// to avoid.
function patchExtensionForIsolatedPort(extDir) {
  const manifestPath = path.join(extDir, "manifest.json");
  const manifestSrc = fs.readFileSync(manifestPath, "utf8");
  const cspNeedle = "connect-src https://*.service-now.com https://snutils.com ws://127.0.0.1:1978/";
  if (!manifestSrc.includes(cspNeedle)) {
    fail(
      "manifest.json CSP no longer matches the expected literal — SN-Utils " +
        "likely shipped an update; this patch (and tests/e2e/README.md's " +
        "note about it) needs updating before the harness can run safely.",
    );
  }
  fs.writeFileSync(
    manifestPath,
    manifestSrc.replace(cspNeedle, cspNeedle.replace("1978/", "*/")),
    "utf8",
  );

  const scriptsyncPath = path.join(extDir, "scriptsync.js");
  const scriptsyncSrc = fs.readFileSync(scriptsyncPath, "utf8");
  const wsNeedle = 'ws = new WebSocket("ws://127.0.0.1:1978");';
  if (!scriptsyncSrc.includes(wsNeedle)) {
    fail(
      "scriptsync.js WebSocket setup no longer matches the expected " +
        "literal — SN-Utils likely shipped an update; this patch needs " +
        "updating before the harness can run safely.",
    );
  }
  const wsReplacement =
    "var __snowCliTestPort = new URLSearchParams(window.location.search).get('port') || 1978;\n" +
    "        ws = new WebSocket(\"ws://127.0.0.1:\" + __snowCliTestPort);";
  fs.writeFileSync(scriptsyncPath, scriptsyncSrc.replace(wsNeedle, wsReplacement), "utf8");

  log("patched extension for isolated-port testing");
}

async function launchBrowser(extDir) {
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), "snu-harness-profile-"));
  log("launching headless chromium with SN-Utils extension");
  const context = await chromium.launchPersistentContext(userDataDir, {
    // headless:true makes Playwright pick the extension-incapable
    // chrome-headless-shell binary. Launch the full binary headed and tell
    // *it* to run headless via --headless=new instead (validated fix).
    headless: false,
    args: [
      `--disable-extensions-except=${extDir}`,
      `--load-extension=${extDir}`,
      "--headless=new",
    ],
    // Playwright's own default args include --disable-extensions, which
    // wins over --disable-extensions-except above and silently prevents the
    // extension's service worker from ever starting (validated fix).
    ignoreDefaultArgs: ["--disable-extensions"],
  });

  let sw = context.serviceWorkers()[0];
  if (!sw) {
    sw = await context.waitForEvent("serviceworker", { timeout: 15_000 });
  }
  const extensionId = new URL(sw.url()).host;
  log(`extension loaded, id=${extensionId}`);
  return { context, extensionId, userDataDir };
}

async function loginToServiceNow(context, instanceUrl, username, password) {
  const page = await context.newPage();
  const loginUrl = new URL("/login.do", instanceUrl).toString();
  log(`logging into ${instanceUrl}`);
  await page.goto(loginUrl, { waitUntil: "domcontentloaded" });

  // ServiceNow's standard basic-auth login form field ids — stable across
  // instances unless SSO is configured (out of scope: matches the
  // --auth-method basic assumption these scenarios already make elsewhere).
  await page.waitForSelector("#user_name", { timeout: 20_000 });
  await page.fill("#user_name", username);
  await page.fill("#user_password", password);
  await Promise.all([
    page.waitForNavigation({ waitUntil: "domcontentloaded", timeout: 30_000 }),
    page.click("#sysverb_login"),
  ]);
  log("ServiceNow login submitted");
  return page;
}

async function openScriptSyncTab(context, extensionId, wsPort) {
  const page = await context.newPage();
  const helperUrl = `chrome-extension://${extensionId}/scriptsync.html?port=${wsPort}`;
  log(`opening ScriptSync helper tab: ${helperUrl}`);
  await page.goto(helperUrl);
  // scriptsync.js connects on DOMContentLoaded and retries every 1s.
  await page.waitForTimeout(3000);
}

async function main() {
  const instanceUrl = requireEnv("SNOW_E2E_INSTANCE_URL");
  const username = requireEnv("SNOW_E2E_USERNAME");
  const password = requireEnv("SNOW_E2E_PASSWORD");
  const scratchDir = requireEnv("SNU_HARNESS_SCRATCH_DIR");
  const wsPort = process.env.SNU_HARNESS_WS_PORT || "19178";
  const readyFile =
    process.env.SNU_HARNESS_READY_FILE || path.join(scratchDir, "snu-harness-ready.json");

  const extDir = await downloadExtension(scratchDir);
  patchExtensionForIsolatedPort(extDir);

  const { context, extensionId } = await launchBrowser(extDir);

  let loginOk = true;
  try {
    await loginToServiceNow(context, instanceUrl, username, password);
  } catch (err) {
    loginOk = false;
    log(`WARNING: ServiceNow login failed, continuing bridge-only: ${err.message}`);
  }

  await openScriptSyncTab(context, extensionId, wsPort);

  fs.writeFileSync(
    readyFile,
    JSON.stringify(
      {
        ready: true,
        pid: process.pid,
        ws_port: wsPort,
        login_ok: loginOk,
        token_capture: "not_implemented",
      },
      null,
      2,
    ),
  );
  log(`ready signal written to ${readyFile}`);

  let shuttingDown = false;
  const shutdown = async () => {
    if (shuttingDown) return;
    shuttingDown = true;
    log("shutting down");
    await context.close().catch(() => {});
    process.exit(0);
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);

  await new Promise(() => {}); // block until a signal arrives
}

main().catch((err) => {
  fail(err.stack || String(err));
});
