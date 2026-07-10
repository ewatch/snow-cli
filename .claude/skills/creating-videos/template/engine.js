/* =============================================================================
   Nori HTML Motion Engine
   -----------------------------------------------------------------------------
   A tiny, dependency-free, *deterministic* animation engine.

   Core idea: every visual is a pure function of a clock `t` (seconds). Nothing
   animates "by itself" — the player computes the exact state for a given time
   and writes it to the DOM. That means:
     - live playback = advance t with requestAnimationFrame and seek()
     - any single frame = seek(t) and read the DOM
     - rendering to MP4 later = seek(t) headless, screenshot, repeat
   ...all from the same code path. No rewrite needed for capture.

   Design space is a fixed 1920x1080 stage, scaled to fit the viewport.
   ========================================================================== */

(function () {
  'use strict';

  // ---- tiny DOM helper -----------------------------------------------------
  // h('div.card#hero', {style:{...}, text:'hi', attrs:{...}}, [children])
  function h(spec, props, children) {
    props = props || {};
    children = children || [];
    let tag = 'div', id = null;
    const classes = [];
    spec.replace(/([.#]?[^.#]+)/g, (m) => {
      if (m[0] === '.') classes.push(m.slice(1));
      else if (m[0] === '#') id = m.slice(1);
      else tag = m;
      return m;
    });
    const el = document.createElement(tag);
    if (id) el.id = id;
    if (classes.length) el.className = classes.join(' ');
    if (props.class) el.className = (el.className ? el.className + ' ' : '') + props.class;
    if (props.text != null) el.textContent = props.text;
    if (props.html != null) el.innerHTML = props.html;
    if (props.style) for (const k in props.style) el.style[k] = props.style[k];
    if (props.attrs) for (const k in props.attrs) el.setAttribute(k, props.attrs[k]);
    for (const c of [].concat(children)) {
      if (c == null) continue;
      el.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
    }
    return el;
  }

  // SVG helper (namespaced)
  const SVGNS = 'http://www.w3.org/2000/svg';
  function s(tag, attrs, children) {
    const el = document.createElementNS(SVGNS, tag);
    if (attrs) for (const k in attrs) el.setAttribute(k, attrs[k]);
    for (const c of [].concat(children || [])) {
      if (c == null) continue;
      el.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
    }
    return el;
  }

  // ---- example logo helper (two overlapping rounded squares) ---------------
  // A sample brand-mark helper: a darker back square + a brighter front square,
  // overlapping. Built from divs so it animates cleanly with the engine. Returns
  // the root plus the two squares so a scene can stagger them in / pulse / exit.
  // Re-theme (or replace) this for your brand's mark.
  function makeNoriLogo(opts) {
    opts = opts || {};
    const size = opts.size || 140;
    const sq = Math.round(size * 0.556);          // square side (20/36 of the mark)
    const r = Math.round(size * 0.12);            // corner radius
    const backC = opts.back || '#1d8f3f';
    const frontC = opts.front || '#42be65';
    // position:relative so children anchor to it; works as a flex child OR when
    // a scene re-positions it absolutely via its place() helper.
    const el = h('div', { style: { position: 'relative', width: size + 'px', height: size + 'px' } });
    const back = h('i', { style: {
      position: 'absolute', display: 'block',
      left: Math.round(size * 0.139) + 'px', top: Math.round(size * 0.194) + 'px',
      width: sq + 'px', height: sq + 'px', borderRadius: r + 'px', background: backC,
    }});
    const front = h('i', { style: {
      position: 'absolute', display: 'block',
      left: Math.round(size * 0.333) + 'px', top: Math.round(size * 0.333) + 'px',
      width: sq + 'px', height: sq + 'px', borderRadius: r + 'px', background: frontC,
      boxShadow: opts.glow === false ? 'none' : '0 0 ' + Math.round(size * 0.18) + 'px rgba(66,190,101,0.5)',
    }});
    el.appendChild(back); el.appendChild(front);
    return { el, back, front };
  }

  // ---- easing --------------------------------------------------------------
  const EASE = {
    linear: t => t,
    inQuad: t => t * t,
    outQuad: t => 1 - (1 - t) * (1 - t),
    inOutQuad: t => (t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2),
    inCubic: t => t * t * t,
    outCubic: t => 1 - Math.pow(1 - t, 3),
    inOutCubic: t => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2),
    outQuart: t => 1 - Math.pow(1 - t, 4),
    outQuint: t => 1 - Math.pow(1 - t, 5),
    inOutQuint: t => (t < 0.5 ? 16 * t * t * t * t * t : 1 - Math.pow(-2 * t + 2, 5) / 2),
    outExpo: t => (t === 1 ? 1 : 1 - Math.pow(2, -10 * t)),
    outBack: t => { const c1 = 1.70158, c3 = c1 + 1; return 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2); },
    outBackSoft: t => { const c1 = 1.0, c3 = c1 + 1; return 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2); },
    outElastic: t => { const c4 = (2 * Math.PI) / 3; return t === 0 ? 0 : t === 1 ? 1 : Math.pow(2, -10 * t) * Math.sin((t * 10 - 0.75) * c4) + 1; },
    inOutBack: t => { const c1 = 1.70158, c2 = c1 * 1.525; return t < 0.5 ? (Math.pow(2 * t, 2) * ((c2 + 1) * 2 * t - c2)) / 2 : (Math.pow(2 * t - 2, 2) * ((c2 + 1) * (t * 2 - 2) + c2) + 2) / 2; },
  };
  function ease(name, t) { return (EASE[name] || EASE.outCubic)(t); }

  const clamp01 = v => (v < 0 ? 0 : v > 1 ? 1 : v);
  const lerp = (a, b, p) => a + (b - a) * p;

  // Which channels are transform-related (composed into one transform string).
  const TRANSFORM_CH = { x: 'px', y: 'px', scale: '', scaleX: '', scaleY: '', rotate: 'deg' };
  const TRANSFORM_DEFAULTS = { x: 0, y: 0, scale: 1, scaleX: 1, scaleY: 1, rotate: 0 };

  // ---- Scene builder context ----------------------------------------------
  // Passed to a scene's build() fn. Collects DOM, tween tracks, captions.
  class Scene {
    constructor(def) {
      this.id = def.id;
      this.title = def.title || def.id;
      this.dur = def.dur;
      this.bg = def.bg || null;          // optional background css
      this._build = def.build;
      this.root = null;                  // 1920x1080 scene container
      this.tracks = [];                  // [{el, ch, unit, segs:[{start,dur,from,to,ease}]}]
      this.captions = [];                // [{text, start, end, cls}]
      this._byKey = new Map();           // el+ch -> track
      this._built = false;
    }

    // create + append an element to the scene root
    node(spec, props, children) { const el = h(spec, props, children); this.root.appendChild(el); return el; }
    add(el) { this.root.appendChild(el); return el; }
    h(spec, props, children) { return h(spec, props, children); }   // create, not appended
    svg(tag, attrs, children) { return s(tag, attrs, children); }

    _track(el, ch, unit) {
      // Capture any pre-existing inline transform (e.g. CSS centering like
      // translate(-50%,-50%)) ONCE, so engine motion composes with it instead
      // of clobbering it. _track only runs during build(), before any seek().
      if ((ch in TRANSFORM_CH) && el.__baseTf === undefined) el.__baseTf = el.style.transform || '';
      const key = (el.__nid || (el.__nid = ++Scene._nid)) + ':' + ch;
      let tr = this._byKey.get(key);
      if (!tr) { tr = { el, ch, unit: unit || '', segs: [] }; this._byKey.set(key, tr); this.tracks.push(tr); }
      return tr;
    }

    // Register a timed transition on an element.
    // spec: {start, dur, ease, opacity:[a,b], x:[a,b], y, scale, rotate, blur,
    //        css:{prop:[a,b,'unit']}}
    to(el, spec) {
      const start = spec.start || 0;
      const dur = spec.dur != null ? spec.dur : 1;
      const ez = spec.ease || 'outCubic';
      for (const k in spec) {
        if (k === 'start' || k === 'dur' || k === 'ease' || k === 'css') continue;
        const v = spec[k];
        const pair = Array.isArray(v) ? v : [v, v];
        let unit = '';
        if (k in TRANSFORM_CH) unit = TRANSFORM_CH[k];
        else if (k === 'blur' || k === 'rad') unit = 'px';
        this._track(el, k, unit).segs.push({ start, dur, from: pair[0], to: pair[1], ease: ez });
      }
      if (spec.css) for (const prop in spec.css) {
        const arr = spec.css[prop];
        this._track(el, 'css:' + prop, arr[2] || '').segs.push({ start, dur, from: arr[0], to: arr[1], ease: ez });
      }
      return el;
    }

    // Instant state at a time (0-duration). Good for initial hide/positions.
    set(el, spec, at) { const s2 = Object.assign({ start: at || 0, dur: 0, ease: 'linear' }, spec); return this.to(el, s2); }

    caption(text, start, end, cls) { this.captions.push({ text, start, end: end == null ? this.dur : end, cls: cls || '' }); return this; }

    // ---- evaluation -------------------------------------------------------
    _ensureBuilt(root) {
      if (this._built) return;
      this.root = root;
      if (this.bg) this.root.style.background = this.bg;
      this._build(this);
      this._built = true;
    }

    _valueOf(track, t) {
      const segs = track.segs;
      if (t <= segs[0].start) return segs[0].from;
      let seg = segs[0];
      for (let i = 0; i < segs.length; i++) { if (segs[i].start <= t) seg = segs[i]; else break; }
      const end = seg.start + seg.dur;
      if (t >= end || seg.dur === 0) return seg.to;
      const p = ease(seg.ease, clamp01((t - seg.start) / seg.dur));
      // numeric interpolation; if not numeric, switch at midpoint
      if (typeof seg.from === 'number' && typeof seg.to === 'number') return lerp(seg.from, seg.to, p);
      return p < 1 ? seg.from : seg.to;
    }

    seek(t) {
      // accumulate per-element styles for this frame
      const acc = new Map();
      const get = (el) => { let a = acc.get(el); if (!a) { a = { tx: null, style: {} }; acc.set(el, a); } return a; };
      for (const tr of this.tracks) {
        const v = this._valueOf(tr, t);
        const a = get(tr.el);
        if (tr.ch in TRANSFORM_CH) { if (!a.tx) a.tx = {}; a.tx[tr.ch] = v; }
        else if (tr.ch === 'opacity') a.style.opacity = v;
        else if (tr.ch === 'blur') a.style.filter = `blur(${v}px)`;
        else if (tr.ch.startsWith('css:')) a.style[tr.ch.slice(4)] = (typeof v === 'number' ? v + tr.unit : v);
        else a.style[tr.ch] = v;
      }
      for (const [el, a] of acc) {
        if (a.tx) {
          const d = TRANSFORM_DEFAULTS;
          const x = a.tx.x != null ? a.tx.x : d.x, y = a.tx.y != null ? a.tx.y : d.y;
          const sc = a.tx.scale != null ? a.tx.scale : d.scale;
          const sx = a.tx.scaleX != null ? a.tx.scaleX : sc, sy = a.tx.scaleY != null ? a.tx.scaleY : sc;
          const r = a.tx.rotate != null ? a.tx.rotate : d.rotate;
          const base = el.__baseTf ? el.__baseTf + ' ' : '';
          el.style.transform = `${base}translate3d(${x}px,${y}px,0) rotate(${r}deg) scale(${sx},${sy})`;
        }
        for (const k in a.style) el.style[k] = a.style[k];
      }
    }

    activeCaptions(t) { return this.captions.filter(c => t >= c.start && t < c.end); }
  }
  Scene._nid = 0;

  // ---- Player --------------------------------------------------------------
  const scenes = [];
  function registerScene(def) { scenes.push(new Scene(def)); }

  class Player {
    constructor(opts) {
      this.viewport = opts.viewport;
      this.stage = opts.stage;
      this.captionBar = opts.captionBar;
      this.scenes = scenes;
      this.t = 0;                 // global time
      this.playing = false;
      this.rate = 1;
      this._raf = null;
      this._last = 0;
      this._curScene = -1;
      this._sceneRoots = [];
      this.onTick = opts.onTick || null;

      // pre-create a root per scene (built lazily on first entry)
      for (let i = 0; i < this.scenes.length; i++) {
        const r = h('div.scene', { attrs: { 'data-scene': this.scenes[i].id } });
        r.style.display = 'none';
        this.stage.appendChild(r);
        this._sceneRoots.push(r);
      }
      this._layout();
      window.addEventListener('resize', () => this._layout());
    }

    get total() { return this.scenes.reduce((a, s2) => a + s2.dur, 0); }
    sceneStart(i) { let acc = 0; for (let k = 0; k < i; k++) acc += this.scenes[k].dur; return acc; }
    sceneAtGlobal(t) {
      let acc = 0;
      for (let i = 0; i < this.scenes.length; i++) {
        if (t < acc + this.scenes[i].dur || i === this.scenes.length - 1) return { i, local: t - acc };
        acc += this.scenes[i].dur;
      }
      return { i: 0, local: 0 };
    }

    _layout() {
      const k = Math.min(window.innerWidth / 1920, window.innerHeight / 1080);
      this.stage.style.transform = `translate(-50%,-50%) scale(${k})`;
    }

    _showScene(i) {
      if (i === this._curScene) return;
      if (this._curScene >= 0) this._sceneRoots[this._curScene].style.display = 'none';
      this._curScene = i;
      const sc = this.scenes[i], root = this._sceneRoots[i];
      sc._ensureBuilt(root);
      root.style.display = 'block';
    }

    seekGlobal(t) {
      t = Math.max(0, Math.min(t, this.total - 0.0001));
      this.t = t;
      const { i, local } = this.sceneAtGlobal(t);
      this._showScene(i);
      this.scenes[i].seek(local);
      this._renderCaptions(this.scenes[i], local);
      if (this.onTick) this.onTick(t, i, local);
    }

    _renderCaptions(sc, local) {
      if (!this.captionBar) return;
      const cues = sc.activeCaptions(local);
      this.captionBar.innerHTML = '';
      for (const c of cues) {
        const line = h('div.caption-line' + (c.cls ? '.' + c.cls : ''), { text: c.text });
        this.captionBar.appendChild(line);
      }
      this.captionBar.style.opacity = cues.length ? '1' : '0';
    }

    play() {
      if (this.playing) return;
      this.playing = true;
      this._last = performance.now();
      const loop = (now) => {
        if (!this.playing) return;
        const dt = (now - this._last) / 1000 * this.rate;
        this._last = now;
        let nt = this.t + dt;
        if (nt >= this.total) { nt = this.total - 0.0001; this.pause(); }
        this.seekGlobal(nt);
        this._raf = requestAnimationFrame(loop);
      };
      this._raf = requestAnimationFrame(loop);
    }
    pause() { this.playing = false; if (this._raf) cancelAnimationFrame(this._raf); this._raf = null; }
    toggle() { this.playing ? this.pause() : this.play(); }
    restart() { this.pause(); this.seekGlobal(0); }
    nextScene() { const { i } = this.sceneAtGlobal(this.t); this.pause(); this.seekGlobal(this.sceneStart(Math.min(i + 1, this.scenes.length - 1))); }
    prevScene() { const { i, local } = this.sceneAtGlobal(this.t); this.pause(); const target = local > 0.4 ? i : Math.max(i - 1, 0); this.seekGlobal(this.sceneStart(target)); }
  }

  // ---- public API ----------------------------------------------------------
  window.Nori = {
    h, s, EASE, ease,
    noriLogo: makeNoriLogo,
    scene: registerScene,
    scenes,
    Player,
    _Scene: Scene,
  };
})();
