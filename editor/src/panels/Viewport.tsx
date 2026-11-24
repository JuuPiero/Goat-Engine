// Scene / Game view: one canvas the engine draws into (kept mounted: a WebGPU surface is tied
// to its canvas). Scene = the open scene through the editor camera; Game = Play mode.
//   right drag / Alt+left drag: orbit · middle drag: pan · wheel: zoom · F: frame selection
//   click: select · Ctrl+S: save scene · Play mode: keyboard = 3DS buttons (K/J/I/U, WASD, arrows, Enter)
import { useEffect, useRef, useState } from 'react';
import { frameSelected, selectEntities } from '../selection';
import { afterChange } from '../commands';
import { getEngine, saveScene, startEngine, startPlay, stopPlay } from '../engine';
import { getState, log, setState, useStore } from '../store';

type LogLevel = 'info' | 'warn' | 'error';

/// Game view sizes (Unity's aspect / resolution menu). 3ds = the two 3DS screens (chosen when
/// Play starts); an aspect fits the largest box into the view; a resolution renders that many
/// pixels, scaled down to fit.
const GAME_SIZES: { id: string; label: string; aspect?: number; px?: [number, number] }[] = [
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
const SIZE_KEY = 'goat.editor.gameSize';
function savedSize() {
  try {
    return localStorage.getItem(SIZE_KEY) ?? '3ds';
  } catch {
    return '3ds';
  }
}

/// Where the canvas sits in a w×h view for a game size (CSS px), and its pixel size.
export function gameRect(id: string, w: number, h: number, dpr: number) {
  const s = GAME_SIZES.find((g) => g.id === id);
  const aspect = s?.px ? s.px[0] / s.px[1] : s?.aspect;
  if (!aspect) return { x: 0, y: 0, w, h, pw: Math.round(w * dpr), ph: Math.round(h * dpr) };
  const fw = Math.min(w, h * aspect);
  const fh = fw / aspect;
  const [pw, ph] = s?.px ?? [Math.round(fw * dpr), Math.round(fh * dpr)];
  return { x: Math.floor((w - fw) / 2), y: Math.floor((h - fh) / 2), w: Math.floor(fw), h: Math.floor(fh), pw, ph };
}

export function Viewport() {
  const canvas = useRef<HTMLCanvasElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const tab = useStore((s) => s.viewTab);
  const playing = useStore((s) => s.playing);
  const scene = useStore((s) => s.scene);
  const dirty = useStore((s) => s.sceneDirty);
  const dual = useStore((s) => s.dual3ds);
  const drop = useStore((s) => s.dropTarget);
  const [stats, setStats] = useState('');
  const [error, setError] = useState<string | null>(null);
  const drag = useRef<{ x: number; y: number; mode: 'orbit' | 'pan' | 'click' | 'gizmo'; moved: boolean } | null>(null);
  const [gameSize, setGameSize] = useState(savedSize);
  const sizing = useRef({ game: false, size: gameSize });
  sizing.current = { game: tab === 'game' && playing, size: gameSize };
  const relayout = useRef<() => void>(() => {});
  // Unity's tools: Q hand (left drag pans), W move, E rotate, R scale; Z Pivot / Center, X Local / Global
  const [gizmo, setGizmo] = useState<'hand' | 'move' | 'rotate' | 'scale'>('move');
  const tool = useRef(gizmo);
  tool.current = gizmo;
  const setMode = (m: 'hand' | 'move' | 'rotate' | 'scale') => {
    setGizmo(m);
    if (m !== 'hand') getEngine()?.set_gizmo_mode(m);
  };
  const [handles, setHandles] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('goat.editor.handles') ?? '') as { local: boolean; center: boolean };
    } catch {
      return { local: false, center: false };
    }
  });
  useEffect(() => {
    getEngine()?.set_tool_handles(handles.local, handles.center, 0.25, 15, 0.1);
    try {
      localStorage.setItem('goat.editor.handles', JSON.stringify(handles));
    } catch {
      /* not remembered */
    }
  }, [handles]);
  const handlesRef = useRef(handles);
  handlesRef.current = handles;
  const toggleHandles = useRef((k: 'local' | 'center') => setHandles((h) => ({ ...h, [k]: !h[k] })));

  useEffect(() => {
    const c = canvas.current!;
    let raf = 0;
    let last = performance.now();
    let frames = 0;
    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      const box = c.parentElement!;
      const { game, size } = sizing.current;
      const r = gameRect(game ? size : 'free', Math.max(1, box.clientWidth), Math.max(1, box.clientHeight), dpr);
      Object.assign(c.style, { left: `${r.x}px`, top: `${r.y}px`, width: `${r.w}px`, height: `${r.h}px` });
      const w = Math.max(1, r.pw);
      const h = Math.max(1, r.ph);
      if (c.width !== w || c.height !== h) {
        c.width = w;
        c.height = h;
        getEngine()?.resize(w, h);
      }
    };
    relayout.current = resize;
    const ro = new ResizeObserver(resize);
    ro.observe(c.parentElement!);
    resize();
    startEngine(c)
      .then(() => {
        resize();
        getEngine()?.set_tool_handles(handlesRef.current.local, handlesRef.current.center, 0.25, 15, 0.1);
        const loop = (now: number) => {
          const e = getEngine();
          if (e) {
            try {
              e.frame((now - last) / 1000);
            } catch (err) {
              setError(String(err));
              return;
            }
            if (++frames % 20 === 0) setStats(e.stats());
            // engine log lines (script errors, warnings) → the Console
            const lines = e.take_logs();
            if (lines !== '[]') for (const [level, text] of JSON.parse(lines) as [LogLevel, string][]) log(text, level);
            if (!e.is_playing() && getState().playing) setState({ playing: false, viewTab: 'scene' });
          }
          last = now;
          raf = requestAnimationFrame(loop);
        };
        raf = requestAnimationFrame(loop);
      })
      .catch((err) => {
        setError(String(err));
        log(`engine: ${err}`, 'error');
      });
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, []);

  useEffect(() => relayout.current(), [tab, playing, gameSize]);
  // the remembered size decides the Play layout too
  useEffect(() => setState({ dual3ds: savedSize() === '3ds' }), []);

  // keyboard: play-mode input, F, Ctrl+S
  useEffect(() => {
    const isTyping = () => ['INPUT', 'TEXTAREA'].includes((document.activeElement?.tagName ?? '').toUpperCase());
    const down = (ev: KeyboardEvent) => {
      const e = getEngine();
      if (!e || isTyping()) return;
      if (e.is_playing()) {
        e.key(ev.code, true);
        if (ev.code === 'Escape') stopPlay();
        if (!ev.ctrlKey) ev.preventDefault();
        return;
      }
      if (!wrap.current?.matches(':hover')) return;
      if (ev.ctrlKey || ev.metaKey || ev.altKey) return;
      if (ev.code === 'KeyF') frameSelected();
      if (ev.code === 'KeyQ') setMode('hand');
      if (ev.code === 'KeyZ') toggleHandles.current('center');
      if (ev.code === 'KeyX') toggleHandles.current('local');
      if (ev.code === 'KeyW') setMode('move');
      if (ev.code === 'KeyE') setMode('rotate');
      if (ev.code === 'KeyR') setMode('scale');
    };
    const up = (ev: KeyboardEvent) => getEngine()?.is_playing() && getEngine()!.key(ev.code, false);
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    return () => {
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
    };
  }, []);

  const onPointerDown = (ev: React.PointerEvent) => {
    const e = getEngine();
    if (!e || e.is_playing()) return;
    const r = canvas.current!.getBoundingClientRect();
    const onGizmo = ev.button === 0 && !ev.altKey && tool.current !== 'hand' && e.gizmo_begin((ev.clientX - r.left) / r.width, (ev.clientY - r.top) / r.height, r.width, r.height);
    // the drag is one Undo step (recorded before the first change)
    if (onGizmo) e.record_undo(gizmo === 'move' ? 'Move' : gizmo === 'rotate' ? 'Rotate' : 'Scale');
    const mode = onGizmo ? 'gizmo' : ev.button === 2 || (ev.button === 0 && ev.altKey) ? 'orbit' : ev.button === 1 || (ev.button === 0 && tool.current === 'hand') ? 'pan' : 'click';
    drag.current = { x: ev.clientX, y: ev.clientY, mode, moved: false };
    (ev.target as HTMLElement).setPointerCapture(ev.pointerId);
    ev.preventDefault();
  };
  const onPointerMove = (ev: React.PointerEvent) => {
    const d = drag.current;
    const e = getEngine();
    if (!d || !e) return;
    const dx = ev.clientX - d.x;
    const dy = ev.clientY - d.y;
    if (Math.abs(dx) + Math.abs(dy) > 2) d.moved = true;
    if (d.mode === 'orbit') e.orbit(dx, dy);
    if (d.mode === 'pan') e.pan(dx, dy);
    if (d.mode === 'gizmo') {
      const r = canvas.current!.getBoundingClientRect();
      e.gizmo_drag(dx, dy, r.width, r.height, ev.ctrlKey || ev.metaKey);
    }
    d.x = ev.clientX;
    d.y = ev.clientY;
  };
  const onPointerUp = (ev: React.PointerEvent) => {
    const d = drag.current;
    drag.current = null;
    const e = getEngine();
    if (d?.mode === 'gizmo' && e) {
      e.gizmo_end();
      if (d.moved) afterChange();
      return;
    }
    if (!d || !e || d.mode !== 'click' || d.moved) return;
    const r = canvas.current!.getBoundingClientRect();
    const id = e.pick((ev.clientX - r.left) / r.width, (ev.clientY - r.top) / r.height) ?? null;
    // Ctrl / Shift + click: add to / remove from the selection (Unity)
    if ((ev.ctrlKey || ev.shiftKey || ev.metaKey) && id) {
      const cur = getState().entities;
      selectEntities(cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]);
    } else selectEntities(id ? [id] : []);
  };

  const showCanvas = tab === 'scene' ? !playing : playing;
  return (
    <div className="viewport" ref={wrap}>
      <div className="viewport-bar">
        <div className="seg">
          <button className={tab === 'scene' ? 'on' : ''} onClick={() => setState({ viewTab: 'scene' })}>
            Scene
          </button>
          <button className={tab === 'game' ? 'on' : ''} onClick={() => setState({ viewTab: 'game' })}>
            Game
          </button>
        </div>
        <div className="seg" title="Tools: Q hand · W move · E rotate · R scale (Ctrl-drag snaps)">
          {(['hand', 'move', 'rotate', 'scale'] as const).map((m) => (
            <button key={m} className={gizmo === m ? 'on' : ''} onClick={() => setMode(m)} title={{ hand: 'View Tool (Q)', move: 'Move Tool (W)', rotate: 'Rotate Tool (E)', scale: 'Scale Tool (R)' }[m]}>
              {m === 'hand' ? '✋' : m === 'move' ? '✥' : m === 'rotate' ? '⟳' : '⤢'}
            </button>
          ))}
        </div>
        <div className="seg">
          <button onClick={() => toggleHandles.current('center')} title="Tool handle position: Pivot / Center (Z)">
            {handles.center ? 'Center' : 'Pivot'}
          </button>
          <button onClick={() => toggleHandles.current('local')} title="Tool handle rotation: Local / Global (X)">
            {handles.local ? 'Local' : 'Global'}
          </button>
        </div>
        <span className="dim scene-name">
          {scene ? `${scene}${dirty ? ' *' : ''}` : 'no scene open — double-click a .scene.json'}
        </span>
        <div className="spacer" />
        <select
          className="game-size"
          title="Game view size"
          value={dual ? '3ds' : gameSize === '3ds' ? 'free' : gameSize}
          onChange={(e) => {
            const id = e.target.value;
            if (playing && (id === '3ds') !== dual) return log('the 3DS screen layout is chosen when Play starts', 'warn');
            setGameSize(id);
            setState({ dual3ds: id === '3ds' });
            try {
              localStorage.setItem(SIZE_KEY, id);
            } catch {
              /* not remembered */
            }
          }}
        >
          {GAME_SIZES.map((g) => (
            <option key={g.id} value={g.id}>
              {g.label}
            </option>
          ))}
        </select>
        <button className={`play${playing ? ' on' : ''}`} onClick={() => (playing ? stopPlay() : void startPlay())} title="Play / Stop (Esc)">
          {playing ? '■' : '▶'}
        </button>
        <button disabled={!scene || playing} onClick={() => void saveScene()} title="Save scene (Ctrl+S)">
          Save
        </button>
        <span className="dim stats">{stats}</span>
      </div>
      <div className={`viewport-canvas${drop === 'scene' ? ' drop' : ''}`} data-drop="scene">
        <canvas
          ref={canvas}
          // inherit when shown: dockview hides the whole panel while another tab of its group is active
          style={{ visibility: showCanvas ? undefined : 'hidden' }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onWheel={(ev) => getEngine()?.zoom(ev.deltaY)}
          onContextMenu={(e) => e.preventDefault()}
        />
        {!showCanvas && <div className="viewport-msg">{tab === 'game' ? 'Press ▶ to play' : 'Playing — see the Game tab'}</div>}
        {error && <div className="viewport-msg error">{error}</div>}
      </div>
    </div>
  );
}
