---
name: creating-videos
description: Use this skill to create animated explainer videos (Kurzgesagt-style motion graphics) built entirely in HTML/CSS/JS — a deterministic, seekable scene engine plus headless-Chrome frame capture, with an optional MP4 render. No video editor required.
---

<required>
CRITICAL: Add the following steps to your Todo list using TodoWrite.

- Announce "I am using the creating-videos skill."
- Research the subject.
  - Search for relevant skills using Glob/Grep in `{{skills_dir}}/`
  - Use the nori-web-researcher for content.
- Propose a few video styles that would fit, and ask me for style approval or for an alternative.
  <system-reminder> You are making the video with html, so focus on minimalist animators instead of hyperrealism. 3blue1brown, kurzgesagt, zero punctuation, xkcd, etc. are all good foundations. </system-reminder>
- If relevant, research the brand.
  - Search in my directory for other visual content I have made. Look for other decks and slides and pull those if necessary.
- Ask how long the video should be.
- Generate the full narration transcript and write to `transcript.md`.
- Generate the video beats. Every scene should visualize one part of the transcript. Aim for 15 seconds per beat.
- Scaffold the project by copying the bundled template:
  `cp -r {{skills_dir}}/creating-videos/template/* <video-dir>/`
  Make changes as needed.
- Build a style frame + 1–2 fully-animated scenes. Serve it, get the
  my sign-off on look/pacing/feel before mass-producing the rest.
- Author the remaining scenes using {{skills_dir}}/handle-large-tasks. with
  - Lock scene order + indices first with stub files that wire to a central index.html
  - Verify each scene using `node capture.mjs <sceneIndex> <t>` (headless chrome). Read the generated pngs. Fix
    overlaps, off-position elements, and timing. Build a contact sheet with ffmpeg to triage many scenes at once.
- Serve the video for review and iterate on feedback.
<system-reminder> Keep the transcript synced. </system-reminder>
- Record + produce the audio track.
  - If relevant, ask the user to record voice using a microphone.
  - If relevant, ask the user to select backing music.
  - Follow the production steps in the audio section below.
- Align the audio and video.
<system-reminder> This may require changing the transcript the video is using. Treat the audio has ground truth. </system-reminder>
- Render to MP4 (Puppeteer seek → ffmpeg, mux audio).
</required>

# System Design

Make the video out of HTML. A browser is a renderer; motion-graphics video is just a web page that plays itself.

Core design principle: everything is a pure function of a clock `t`. The player computes the exact visual state for a given time and writes it to the DOM.

Do NOT use CSS `@keyframes`, `transition`, `setTimeout`, or bare `requestAnimationFrame` for content motion. Express all motion through the engine.

# Project structure (the bundled template gives you all of this)

```
<video-dir>/
  index.html         player shell + dev controls + deep-link handling (?scene&t&still&hud&play)
  engine.js          the deterministic tween/timeline engine (do not rewrite — reuse)
  brand.css          design tokens, 1920×1080 stage, captions, dev controls — adapt :root
  capture.mjs        headless-Chrome frame grabber (verification + seed of the MP4 renderer)
  SCENE_AUTHORING.md the per-scene contract (engine API, layout rules, gotchas)
```

Each scene is a self-contained file that calls `Nori.scene({ id, title, dur, build(S){…} })`.
**Play order = the order of `<script src="scenes/…">` tags in `index.html`**, NOT the
filenames. Inserting a scene shifts every later scene's capture index by one — re-list
the order with `grep -oE 'scenes/[a-z0-9-]+\.js' index.html | nl -v0` whenever in doubt.

# Authoring scenes

Read `SCENE_AUTHORING.md` (bundled). Give it to every subagent.

In short: build DOM in `build(S)`, then declare timed transitions with
`S.to(el, {start, dur, ease, opacity, x, y, scale, rotate, blur, css})` and
narration with `S.caption(text, start, end)`. Channels compose with CSS centering
automatically.

For MANY scenes, this is a large task — use the handle-large-tasks skill: write a
tight brief per scene (narration + on-screen + concrete choreography with rough
coords/timings), spawn parallel subagents to draft batches, and run the full
visual QA yourself. Tell subagents NOT to capture while others are mid-edit (the
page loads all scenes — a half-written scene breaks everyone's capture).

# Engine gotchas

- Transform composition: the engine prepends any pre-existing inline transform
  (e.g. CSS `translate(-50%,-50%)` centering) so motion composes with centering.
  For big centered blocks, animate an INNER element inside a static centered wrapper.
- Capture overflow: `t` past a scene's `dur` renders the NEXT scene. Index math
  shifts whenever you insert/reorder a scene — re-derive from `index.html`.
- Persistence: don't rely on backgrounded servers/processes (reaped → exit 144);
  use tmux. Don't `dangerouslyDisableSandbox` on the launch — it triggers the reap.
- Subagent races: capturing loads ALL scenes; a mid-write scene breaks it. Lock
  structure with stubs first; have only one editor capturing at a time.

# Audio (record → clean → bed → mux)

You cannot hear audio. Every taste call must be auditioned by the human. Render
level-matched A/B variants to a folder and let them pick. Bundled helpers
(self-contained CLI) live in `{{skill_dir}}audio/`. Do audio work in a scratch
dir, not the video-dir.

- If there is a voice track, de-mouth (lip smacks / clicks):
  `python3 {{skill_dir}}audio/demouth.py in.wav out.wav`, which runs `adeclick=threshold=6`

- De-reverb + denoise. BEFORE any EQ/compression run DeepFilterNet.
  curl the CPU binary, then then `{{skill_dir}}audio/deep-filter --pf voice.wav -o df/`

- Polish.
  `highpass=f=75, equalizer=f=130:t=q:w=1:g=2 (warmth),
  equalizer=f=350:t=q:w=1.1:g=-1.5 (de-mud), equalizer=f=4500:t=q:w=1.4:g=2
  (presence), treble=g=2.5:f=11000 (air), acompressor=threshold=-20dB:ratio=3,
  deesser=i=0.4, loudnorm=I=-16:TP=-1.5:LRA=11`.

- Music bed. `{{skill_dir}}audio/make_music_tracks.py` lays a looped, ducked bed.
  - Source from the YouTube Audio Library / no-copyright channels with `yt-dlp`
    (standalone binary).
  - Loop seamlessly with crossfades, NOT a hard `-stream_loop` seam:
    `ffmpeg -i m.wav -i m.wav -i m.wav -filter_complex "[0][1]acrossfade=d=4[a];[a][2]acrossfade=d=4" bed_loop.wav`
  - `python3 {{skill_dir}}audio/make_music_tracks.py voice.wav FINAL.wav bed_loop.wav --bed-mean -31 --voice-gain 0`
    ducks the bed under the VO (`sidechaincompress`), faint by default.

- Balance + delivery loudness. Set voice-vs-bed by ear (`--voice-gain`,
  negative lowers the voice — which also ducks the bed less, a double win), THEN
  normalize the whole MIX: `ffmpeg -i FINAL.wav -af loudnorm=I=-16:TP=-1.5:LRA=11
  DELIVERY.wav` (preserves the chosen balance, hits spec).
