/* Example scene — shows the engine API. Delete this and write your own. ======
   Pattern: build DOM in build(S), then declare timed transitions with S.to(...)
   and narration with S.caption(...). Every value animates as a function of the
   clock; nothing uses CSS @keyframes/transition/setTimeout. */
Nori.scene({
  id: 'example',
  title: 'Example scene',
  dur: 8,
  build(S) {
    // A static centered wrapper we DON'T animate; we animate the inner blocks.
    const wrap = S.node('div.center-stack');
    const kicker = wrap.appendChild(S.h('div.kicker', { text: 'EXAMPLE' }));
    const title  = wrap.appendChild(S.h('div.headline', { style: { fontSize: '84px' }, text: 'Hello, scene.' }));
    const sub    = wrap.appendChild(S.h('div.subhead', { style: { marginTop: '16px' }, text: 'every visual is a function of the clock t' }));

    // A row of three chips that stagger in.
    const row = wrap.appendChild(S.h('div', { style: { display: 'flex', gap: '20px', marginTop: '48px' } }));
    const colors = ['#40c463', '#4c9be8', '#f0b429'];
    const chips = colors.map((c, i) => row.appendChild(S.h('div.chip', { text: 'item ' + (i + 1), style: { borderColor: c } })));

    // ---- timeline: entrance (staggered) → hold → clean exit drift ----
    S.set(kicker, { opacity: 0 });
    S.to(kicker, { start: 0.2, dur: 0.5, opacity: [0, 1] });
    S.set(title, { opacity: 0, y: 28 });
    S.to(title, { start: 0.3, dur: 0.6, ease: 'outCubic', opacity: [0, 1], y: [28, 0] });
    S.set(sub, { opacity: 0, y: 18 });
    S.to(sub, { start: 0.7, dur: 0.6, ease: 'outCubic', opacity: [0, 1], y: [18, 0] });
    chips.forEach((c, i) => {
      S.set(c, { opacity: 0, scale: 0.7, y: 20 });
      S.to(c, { start: 1.3 + i * 0.18, dur: 0.5, ease: 'outBack', opacity: [0, 1], scale: [0.7, 1], y: [20, 0] });
    });
    // every scene should end near-empty so the cut to the next scene is clean
    S.to(wrap, { start: 6.8, dur: 0.9, ease: 'inOutCubic', opacity: [1, 0], y: [0, -40] });

    // narration caption (the spoken line for this beat, from transcript.md)
    S.caption('This is a caption — the spoken line for this beat.', 0.5, 6.5);
  },
});
