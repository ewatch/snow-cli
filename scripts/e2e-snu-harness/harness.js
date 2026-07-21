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
//      the collision risk). Separately, it widens host_permissions on the same
//      local copy to `<all_urls>` so `chrome.tabs.captureVisibleTab` (the
//      `snu screenshot` path) works without the interactive "click the SN
//      Utils icon" activeTab gesture that a real user would perform — see
//      patchExtensionForScreenshot.
//   3. Launches headless Chromium with the patched extension loaded, using
//      the two flag workarounds validated by hand (see README below).
//   4. Opens the extension's ScriptSync helper tab pointed at the isolated
//      port, before logging in, so the bridge is already live when a
//      session gets pushed over it. This step alone is enough to make
//      `snow-cli snu broker status` report browser_connected: true.
//   5. Logs into SNOW_E2E_INSTANCE_URL with SNOW_E2E_USERNAME/PASSWORD via
//      ServiceNow's standard basic-auth login form.
//   6. Triggers SN-Utils' in-page `/token` session-capture command by calling
//      its own `window.snuSlashCommandShow('/token', true)` entry point
//      directly (the same function the Ctrl+//Alt+/ palette shortcut calls
//      into) rather than simulating keystrokes, which didn't reliably land
//      on the right frame in ServiceNow's Next Experience shell even with
//      every documented precondition satisfied.
//   7. Approves the resulting one-time per-instance connection prompt in the
//      ScriptSync tab (`#instanceapprovediv` / `#instanceallow` — see
//      scriptsync.js): the tab does not forward anything over the
//      WebSocket, including /token, until that prompt is accepted. Since
//      this harness always starts from a fresh browser profile, the prompt
//      appears every run.
//   8. Writes a ready-signal JSON file and blocks until SIGTERM/SIGINT, at
//      which point it closes the browser and exits.
//
// Steps 6-7 were confirmed end to end against a real PDI on 2026-07-17:
// `snow-cli snu query incident` returned real records through the resulting
// browser session. `token_capture` in the ready signal is
// "attempted+approved" on success.

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

/// Grants the local unpacked copy the `<all_urls>` host permission so the
/// `snu screenshot` scenario can run unattended. SN-Utils captures screenshots
/// with `chrome.tabs.captureVisibleTab` (scriptsync.js), which Chrome gates on
/// EITHER a gesture-granted `activeTab` (the "click the SN Utils extension icon
/// on the tab" prompt a real user sees) OR the `<all_urls>` host permission —
/// a narrower host match like the shipped `https://*.service-now.com/*` does
/// NOT satisfy it. There is no reliable way to synthesize the icon-click
/// gesture in headless Chromium, so instead we widen host_permissions on the
/// local-only copy: an unpacked (dev-loaded) extension is granted its manifest
/// host permissions without a prompt, so captureVisibleTab then succeeds with
/// no user interaction. Fails loudly if the shipped host entry moved, rather
/// than silently leaving the scenario unable to capture.
function patchExtensionForScreenshot(extDir) {
  const manifestPath = path.join(extDir, "manifest.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const hosts = manifest.host_permissions;
  const shippedHost = "https://*.service-now.com/*";
  if (!Array.isArray(hosts) || !hosts.includes(shippedHost)) {
    fail(
      "manifest.json host_permissions no longer contains the expected " +
        `${shippedHost} entry — SN-Utils likely shipped an update; the ` +
        "screenshot permission patch (and tests/e2e/README.md's note about " +
        "it) needs updating before the harness can run safely.",
    );
  }
  if (!hosts.includes("<all_urls>")) {
    hosts.push("<all_urls>");
    fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2), "utf8");
    log("patched extension host_permissions with <all_urls> for screenshot capture");
  }
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
  return page;
}

/// The first time an instance pushes anything (including /token) over the
/// bridge, the ScriptSync tab shows a one-time per-instance approval prompt
/// (`#instanceapprovediv`, with `#instanceallow`/`#instanceblock` buttons —
/// see scriptsync.js) and does NOT forward the push over the WebSocket until
/// `#instanceallow` is clicked. Without this, /token can fire correctly on
/// the ServiceNow side and still never reach the broker. Since this harness
/// always starts from a fresh browser profile, this prompt appears every run.
async function approveScriptSyncInstance(scriptSyncPage, timeoutMs = 15_000) {
  try {
    await scriptSyncPage.locator("#instanceapprovediv").waitFor({ state: "visible", timeout: timeoutMs });
    log("ScriptSync tab is prompting for instance approval — clicking Allow");
    await scriptSyncPage.click("#instanceallow");
    return true;
  } catch (err) {
    log(`WARNING: no ScriptSync instance-approval prompt appeared within ${timeoutMs}ms: ${err.message}`);
    return false;
  }
}

/// Triggers SN-Utils' in-page `/token` session-capture command — see the
/// `/token` entry at https://snutils.com/docs/guide/slash-commands/cheatsheet:
/// "Send the g_ck (CSRF) token to VS Code for Script Sync authentication".
/// Normally a user opens the slash palette via Ctrl+/ (Mac) / Alt+/
/// (Windows/Linux, https://snutils.com/docs/guide/slash-commands/getting-started),
/// types "token", and presses Enter. This calls the page's own
/// `snuSlashCommandShow('/token', true)` entry point directly instead —
/// that's what the shortcut itself calls into — since simulated CDP key
/// events didn't reliably land on the right frame in this Next Experience
/// shell (see harness.js history / tests/e2e/README.md).
async function attemptTokenCapture(page) {
  const debugDir = process.env.SNU_HARNESS_DEBUG_DIR;
  try {
    await page.bringToFront();
    // window.snuSlashCommandShow is set up asynchronously after inject.js
    // loads (chrome.storage-backed settings, etc.) — checking for it
    // immediately after login is a race that sometimes loses.
    await page
      .waitForFunction(() => typeof window.snuSlashCommandShow === "function", {
        timeout: 15_000,
      })
      .catch(() => {});
    if (debugDir) {
      await page.screenshot({ path: path.join(debugDir, "before-token.png") }).catch(() => {});
      const diag = await page.evaluate(() => ({
        pathname: location.pathname,
        hasSnusettings: typeof window.snusettings !== "undefined",
        slashoption: window.snusettings?.slashoption,
        hasSlashShow: typeof window.snuSlashCommandShow === "function",
        snuPopupExists: !!document.querySelector("div.snutils"),
        hasGck: typeof window.g_ck !== "undefined" && !!window.g_ck,
        activeElementTag: document.activeElement?.tagName,
      }));
      fs.writeFileSync(path.join(debugDir, "diag-before.json"), JSON.stringify(diag, null, 2));
      log(`diagnostic: ${JSON.stringify(diag)}`);
    }
    // Call the page's own slash-command entry point directly rather than
    // simulating the Ctrl+//Alt+/ keyboard shortcut: in this Next Experience
    // shell, synthetic CDP key events sent to page.keyboard did not
    // reliably land on whichever frame currently holds real input focus,
    // so nothing visibly happened even though every precondition
    // (slashoption on, snuSlashCommandShow defined, g_ck present) checked
    // out. snuSlashCommandShow(cmd, autoRun=true) is what that shortcut
    // itself calls into — it sets the command and dispatches its own
    // internal Enter, sidestepping focus/iframe ambiguity entirely.
    const invoked = await page.evaluate(() => {
      if (typeof window.snuSlashCommandShow !== "function") return false;
      window.snuSlashCommandShow("/token", true);
      return true;
    });
    await page.waitForTimeout(1500);
    if (debugDir) {
      await page.screenshot({ path: path.join(debugDir, "after-token.png") }).catch(() => {});
    }
    if (!invoked) {
      log("WARNING: window.snuSlashCommandShow was not found on the page");
      return "failed";
    }
    log("invoked window.snuSlashCommandShow('/token', true) in the ServiceNow tab");
    return "attempted";
  } catch (err) {
    log(`WARNING: /token keystroke attempt failed: ${err.message}`);
    return "failed";
  }
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
  patchExtensionForScreenshot(extDir);

  const { context, extensionId } = await launchBrowser(extDir);

  // Open the bridge before logging in: /token's push has nowhere to go if
  // the helper tab isn't connected yet.
  const scriptSyncPage = await openScriptSyncTab(context, extensionId, wsPort);

  let loginOk = true;
  let tokenCapture = "skipped (login failed)";
  try {
    const snPage = await loginToServiceNow(context, instanceUrl, username, password);
    tokenCapture = await attemptTokenCapture(snPage);
    if (tokenCapture === "attempted") {
      const approved = await approveScriptSyncInstance(scriptSyncPage);
      tokenCapture = approved ? "attempted+approved" : "attempted (no approval prompt seen)";
    }
  } catch (err) {
    loginOk = false;
    log(`WARNING: ServiceNow login failed, continuing bridge-only: ${err.message}`);
  }

  fs.writeFileSync(
    readyFile,
    JSON.stringify(
      {
        ready: true,
        pid: process.pid,
        ws_port: wsPort,
        login_ok: loginOk,
        token_capture: tokenCapture,
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
