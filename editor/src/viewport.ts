// The engine draws into ONE canvas (a WebGPU surface is tied to its canvas), shared by the Scene
// and the Game panels: the panel on screen adopts it (`attach`). Scene = the edit world through
// the editor camera — or, while playing, the play world through it (Unity's Scene view in Play
// mode); Game = the game's cameras, sized by the Game view's aspect / resolution menu.
import { getEngine, startEngine } from './engine';
import { getState, log, setState } from './store';

export type ViewMode = 'scene' | 'game';

/// Game view sizes (Unity's aspect menu). `3ds` = the two 3DS screens (chosen when Play starts);
/// an aspect fits the largest box into the view; a resolution renders that many pixels, scaled to fit.
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

let canvas: HTMLCanvasElement | null = null;
let host: HTMLElement | null = null;
let mode: ViewMode = 'scene';
let observer: ResizeObserver | null = null;
let loopStarted = false;
const listeners = new Set<(mode: ViewMode, host: HTMLElement | null) => void>();

/// The shared canvas (created on first use).
export function viewCanvas(): HTMLCanvasElement {
  if (!canvas) {
    canvas = document.createElement('canvas');
    canvas.className = 'g-canvas';
    canvas.tabIndex = 0;
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());
  }
  return canvas;
}

export function viewMode(): ViewMode {
  return mode;
}
export function viewHost(): HTMLElement | null {
  return host;
}
export function onViewChange(f: (mode: ViewMode, host: HTMLElement | null) => void): () => void {
  listeners.add(f);
  return () => listeners.delete(f);
}

/// Moves the canvas into `el` (the Scene or Game panel on screen).
export function attach(el: HTMLElement, m: ViewMode) {
  const c = viewCanvas();
  if (host === el && mode === m && c.parentElement === el) return;
  host = el;
  mode = m;
  el.appendChild(c);
  observer?.disconnect();
  observer = new ResizeObserver(() => relayout());
  observer.observe(el);
  getEngine()?.set_scene_view(m === 'scene');
  if (getState().viewTab !== m) setState({ viewTab: m });
  relayout();
  listeners.forEach((f) => f(m, el));
  ensureLoop();
}

/// Sizes the canvas for the current host: the whole view (Scene, Free Aspect) or the game size.
export function relayout() {
  const c = viewCanvas();
  if (!host) return;
  const dpr = window.devicePixelRatio || 1;
  const s = getState();
  const size = mode === 'game' ? (s.dual3ds ? '3ds' : s.gameSize) : 'free';
  const r = gameRect(size === '3ds' ? 'free' : size, Math.max(1, host.clientWidth), Math.max(1, host.clientHeight), dpr, mode === 'game' ? s.gameZoom : 1);
  Object.assign(c.style, { left: `${r.x}px`, top: `${r.y}px`, width: `${r.w}px`, height: `${r.h}px` });
  const w = Math.max(1, r.pw), h = Math.max(1, r.ph);
  if (c.width !== w || c.height !== h) {
    c.width = w;
    c.height = h;
    getEngine()?.resize(w, h);
  }
}

type LogLevel = 'info' | 'warn' | 'error';

/// Starts the engine on the canvas and the frame loop (once).
function ensureLoop() {
  if (loopStarted) return;
  loopStarted = true;
  const c = viewCanvas();
  let last = performance.now();
  let frames = 0;
  startEngine(c)
    .then((e) => {
      relayout();
      e.set_scene_view(mode === 'scene');
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
