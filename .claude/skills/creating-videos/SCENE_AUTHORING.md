# Scene authoring spec (read this fully before writing a scene)

You are writing ONE or more scene files for a HTML motion video.
Every scene is self-contained JS that registers itself with the engine.

Also read `engine.js` (the API) and `brand.css` (tokens). Do not add libraries.
Do not edit `index.html`, `engine.js`, or other scene files — only your own.

---

## The model: everything is a pure function of time `t`

Nothing animates on its own. You declare elements and timed transitions; the
engine computes the exact visual state for a time and writes it to the DOM. So:
**never** use CSS `@keyframes`, `transition`, `setTimeout`, or
`requestAnimationFrame`. Express ALL motion through `S.to(...)`.

The stage is exactly **1920×1080**. Position with absolute px coordinates. Keep
content inside a ~80px safe margin. The caption/subtitle bar lives at the bottom
(~y 940–1040) — keep important visuals above ~y 880.

## Registering a scene

```js
Nori.scene({
  id: 'myid',            // unique, matches the stub you're replacing
  title: 'Short title',  // shown in dev HUD
  dur: 9,                // seconds — KEEP the duration from the stub
  build(S) { /* ... */ },
});
```

`build(S)` runs ONCE. Create DOM, then register transitions + caption cues.

## The `S` builder API

- `S.node(spec, props, children)` → create an element **and append to the scene root**. Returns it.
- `S.h(spec, props, children)` → create WITHOUT appending (for nesting). Returns it.
- `S.svg(tag, attrsObj, children)` → create an SVG element (namespaced).
- `spec` is a tag shorthand: `'div.card.big#hero'` → `<div class="card big" id="hero">`.
- `props`: `{ text, html, style:{…cssCamelCase}, attrs:{…}, class }`.
- `S.set(el, {channel:value | [from,to]}, atTime=0)` → instant state (0-dur). Use for initial hide/offset.
- `S.to(el, { start, dur=1, ease='outCubic', <channels>, css:{prop:[from,to,'unit']} })` → a timed transition.
- `S.caption(text, start, end, cls?)` → subtitle cue (this is the narration stand-in). `cls` ∈ `'aside' | 'shout'`.

### Channels (each value is `[from, to]`, or a bare number = constant)
`opacity`, `x`(px), `y`(px), `scale`, `scaleX`, `scaleY`, `rotate`(deg),
`blur`(px), and arbitrary CSS via `css:{ propName:[from,to,'unit'] }`
(e.g. `css:{ width:[0,560,'px'], strokeDashoffset:[300,0,''] }`).

### Easings
`linear, inQuad, outQuad, inOutQuad, inCubic, outCubic, inOutCubic, outQuart,
outQuint, inOutQuint, outExpo, outBack, outBackSoft, outElastic, inOutBack`.

Defaults that look good: entrances `outBack` (pops) or `outCubic` (slides);
moves/exits `inOutCubic`; line draws `outQuart`.

## Centering composes automatically (important)

If an element has inline `transform: translate(-50%,-50%)` (CSS centering), the
engine **prepends** that base, so animating `scale`/`y`/etc. on a centered
element keeps it centered. Two safe patterns:

1. **Static wrapper, animate inner** (best for big text blocks): an outer div
   centered with `translate(-50%,-50%)` that you NEVER animate, holding inner
   blocks that you DO animate.
2. **Animate the centered element directly** — fine, because the base transform
   is preserved.

## Sizing gotchas

- Monospace char width ≈ `0.60 × fontSize`px. If you build a typewriter/clip
  reveal, size the clip ≥ `chars × 0.60 × fontSize` so text isn't cut off.
- Headline text wraps if wider than its container. At 62px bold, budget ~`0.52 × fontSize` per char. Keep big headlines ≤ ~30 chars per line or widen the wrapper.
