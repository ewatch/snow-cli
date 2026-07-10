/* =============================================================================
   capture.mjs — deterministic frame grabber (verification + seed of renderer)
   -----------------------------------------------------------------------------
   Freezes the player at an exact (scene, localTime) via ?still=1 and screenshots
   it with headless Chrome at native 1920x1080. No npm deps.

   Usage:
     node capture.mjs                      # capture the default frame(s)
     node capture.mjs <sceneIdx> <t> name  # capture one frame
   Output: ./frames/*.png
   ========================================================================== */
import { execFileSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const dir = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.join(dir, 'frames');
mkdirSync(outDir, { recursive: true });

const CHROME = process.env.CHROME || 'google-chrome';
const indexUrl = 'file://' + path.join(dir, 'index.html');

// [name, sceneIndex, localTime] — edit to your scenes' key moments.
const defaults = [
  ['frame-0', 0, 1.0],
];

function grab(name, scene, t) {
  const url = `${indexUrl}?still=1&hud=0&scene=${scene}&t=${t}`;
  const out = path.join(outDir, `${name}.png`);
  const args = [
    '--headless=new',
    '--no-sandbox',
    '--disable-gpu',
    '--hide-scrollbars',
    '--force-device-scale-factor=1',
    '--window-size=1920,1080',
    '--virtual-time-budget=2800',
    '--default-background-color=000000ff',
    `--screenshot=${out}`,
    url,
  ];
  process.stdout.write(`  ${name}  (scene ${scene} @ ${t}s) … `);
  execFileSync(CHROME, args, { stdio: ['ignore', 'ignore', 'ignore'] });
  console.log('ok');
}

const argv = process.argv.slice(2);
if (argv.length >= 2) {
  grab(argv[2] || 'frame', +argv[0], +argv[1]);
} else {
  console.log('Capturing default frames →', outDir);
  for (const [name, scene, t] of defaults) grab(name, scene, t);
  console.log('Done.');
}
