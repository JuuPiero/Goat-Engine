// The Scene and Game views each have a canvas (two WebGPU surfaces of one engine). Scene = the edit
// world through the editor camera — or, while playing, the play world through it (Unity's Scene
// view in Play mode); Game = the game's cameras, sized by the Game view's aspect / resolution menu.
import { getEngine, startEngine } from './engine';
import { getState, log, setState } from './store';

export type ViewMode = 'scene' | 'game';

/// Game view sizes (Unity's aspect menu). `3ds` = the two 3DS screens (chosen when Play starts);
/// an aspect fits the largest box into the view; a resolution renders that many pixels, scaled to fit.
import { simDevice, simLayout } from './simulator';

export const GAME_SIZES: { id: string; label: string; aspect?: number; px?: [number, number] }[] = [
  { id: '3ds', label: 'Nintendo 3DS (400×240 + 320×240)' },
  { id: 'free', label: 'Free Aspect' },
  { id: '16:9', label: '16:9 Landscape', aspect: 16 / 9 },
  { id: '16:10', label: '16:10 Landscape', aspect: 16 / 10 },
  { id: '4:3', label: '4:3 Landscape', aspect: 4 / 3 },
  { id: '9:16', label: '9:16 Portrait', aspect: 9 / 16 },
  { id: '1920x1080', label: 'Full HD (1920×1080)', px: [1920, 1080] },
  { id: '1280x720', label: 'HD (1280×720)', px: [1280, 720] },
  { id: '800x480', label: 'WVGA (800×480)', px: [800, 480] },
];

/// Where the canvas sits in a `w`×`h` view for a game size and zoom (CSS px), and its pixel size.
export function gameRect(id: string, w: number, h: number, dpr: number, zoom = 1) {
  const s = GAME_SIZES.find((g) => g.id === id);
  const aspect = s?.px ? s.px[0] / s.px[1] : s?.aspect;
  if (!aspect) return { x: 0, y: 0, w, h, pw: Math.round(w * dpr), ph: Math.round(h * dpr) };
  const fw = Math.min(w, h * aspect) * zoom;
  const fh = fw / aspect;
  const [pw, ph] = s?.px ?? [Math.round(fw * dpr), Math.round(fh * dpr)];
  return { x: Math.floor((w - fw) / 2), y: Math.floor((h - fh) / 2), w: Math.floor(fw), h: Math.floor(fh), pw, ph };
}

// Two canvases (Unity: the Scene and Game views are independent windows, both drawn every
// frame): the Scene canvas is the engine's first surface, the Game canvas a second one.
const canvases: Record<ViewMode, HTMLCanvasElement | null> = { scene: null, game: null };
const hosts: Record<ViewMode, HTMLElement | null> = { scene: null, game: null };
const visible: Record<ViewMode, boolean> = { scene: false, game: false };
const observers: Record<ViewMode, ResizeObserver | null> = { scene: null, game: null };
let loopStarted = false;
let gameAttached = false;
const listeners = new Set<(mode: ViewMode, host: HTMLElement | null) => void>();

/// A view's canvas (created on first use).
export function viewCanvas(m: ViewMode = 'scene'): HTMLCanvasElement {
  let c = canvases[m];
  if (!c) {
    c = document.createElement('canvas');
    c.className = 'g-canvas';
    c.tabIndex = 0;
    c.width = 16;
    c.height = 16;
    c.addEventListener('contextmenu', (e) => e.preventDefault());
    canvases[m] = c;
  }
  return c;
}

/// The view the keyboard / last interaction belongs to (the Game view while playing when shown).
export function viewMode(): ViewMode {
  return getState().playing && visible.game ? 'game' : 'scene';
}
export function viewHost(m: ViewMode = 'scene'): HTMLElement | null {
  return hosts[m];
}
export function isViewVisible(m: ViewMode): boolean {
  return visible[m];
}
export function onViewChange(f: (mode: ViewMode, host: HTMLElement | null) => void): () => void {
  listeners.add(f);
  return () => listeners.delete(f);
}

/// Puts a view's canvas into `el` (its panel) and tells the engine what's on screen.
export function attach(el: HTMLElement, m: ViewMode) {
  const c = viewCanvas(m);
  if (c.parentElement !== el) el.appendChild(c);
  if (hosts[m] !== el) {
    hosts[m] = el;
    observers[m]?.disconnect();
    observers[m] = new ResizeObserver(() => relayout(m));
    observers[m]!.observe(el);
  }
  setVisible(m, true);
  relayout(m);
  listeners.forEach((f) => f(m, el));
  ensureLoop();
}

/// A view's panel was hidden (another tab in its group) or shown.
export function setVisible(m: ViewMode, on: boolean) {
  visible[m] = on;
  getEngine()?.set_views(visible.scene, visible.game && gameAttached);
  if (on && getState().viewTab !== m) setState({ viewTab: m });
}

/// Sizes a view's canvas: the whole view (Scene, Free Aspect) or the game size.
export function relayout(m: ViewMode = 'scene') {
  const c = viewCanvas(m);
  const host = hosts[m];
  if (!host) return;
  const dpr = window.devicePixelRatio || 1;
  const s = getState();
  const size = m === 'game' ? (s.dual3ds ? '3ds' : s.gameSize) : 'free';
  const [hw, hh] = [Math.max(1, host.clientWidth), Math.max(1, host.clientHeight)];
  let r = gameRect(size === '3ds' ? 'free' : size, hw, hh, dpr, m === 'game' ? s.gameZoom : 1);
  // Device Simulator: the canvas spans the device's screens, the device picture is drawn over it
  const device = m === 'game' && s.gameMode === 'simulator' ? host.querySelector<SVGElement>('.g-sim-device') : null;
  if (device) {
    const L = simLayout(simDevice(s.simDevice), hw, hh, s.gameZoom);
    Object.assign(device.style, { left: `${L.device.x}px`, top: `${L.device.y}px`, width: `${L.device.w}px`, height: `${L.device.h}px` });
    const c = L.canvas;
    r = { x: c.x, y: c.y, w: c.w, h: c.h, pw: Math.round(c.w * dpr), ph: Math.round(c.h * dpr) };
  }
  Object.assign(c.style, { left: `${r.x}px`, top: `${r.y}px`, width: `${r.w}px`, height: `${r.h}px` });
  const w = Math.max(1, r.pw), h = Math.max(1, r.ph);
  if (c.width !== w || c.height !== h) {
    c.width = w;
    c.height = h;
    const e = getEngine();
    if (m === 'scene') e?.resize(w, h);
    else if (gameAttached) e?.resize_game(w, h);
  }
}
type LogLevel = 'info' | 'warn' | 'error';

/// Starts the engine on the canvas and the frame loop (once).
/// Drawings over the views (handles, outlines, the axis gizmo): run right after each engine
/// frame, in the same animation frame — a separate requestAnimationFrame ran BEFORE the engine's
/// and showed the previous frame's positions (gizmos trailing moving objects).
export const afterFrame = new Set<() => void>();

function ensureLoop() {
  if (loopStarted) return;
  loopStarted = true;
  const c = viewCanvas('scene');
  let last = performance.now();
  let frames = 0;
  startEngine(c)
    .then((e) => {
      relayout('scene');
      // the Game view's canvas: a second surface (older engines: one shared canvas)
      try {
        relayout('game');
        e.attach_game_canvas(viewCanvas('game'));
        gameAttached = true;
        const g = viewCanvas('game');
        e.resize_game(g.width, g.height);
      } catch (err) {
        log(`game view: ${err}`, 'warn');
      }
      e.set_views(visible.scene, visible.game && gameAttached);
      setState({ engineReady: true });
      const loop = (now: number) => {
        const eng = getEngine();
        if (eng) {
          try {
            eng.frame((now - last) / 1000);
          } catch (err) {
            setState({ engineError: String(err) });
            return;
          }
          if (++frames % 20 === 0) setState({ stats: eng.stats() });
          // engine log lines (script errors, warnings) → the Console
          const lines = eng.take_logs();
          if (lines !== '[]') for (const [level, text] of JSON.parse(lines) as [LogLevel, string][]) log(text, level);
          if (eng.is_playing()) {
            // console answers, Debug.break / Error Pause
            const answers = eng.console_results();
            if (answers !== '[]') for (const r of JSON.parse(answers) as { text: string; ok: boolean }[]) log(`< ${r.text}`, r.ok ? 'info' : 'error');
            const why = eng.take_pause_reason();
            if (why) {
              setState({ paused: true });
              log(`paused: ${why}`, 'warn');
            }
          }
          if (!eng.is_playing() && getState().playing) setState({ playing: false, paused: false });
        }
        for (const f of afterFrame) {
          try {
            f();
          } catch (err) {
            console.error(err);
          }
        }
        last = now;
        requestAnimationFrame(loop);
      };
      requestAnimationFrame(loop);
    })
    .catch((err) => {
      setState({ engineError: String(err) });
      log(`engine: ${err}`, 'error');
    });
}
