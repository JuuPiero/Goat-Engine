// Scene / Game view: one canvas the engine draws into (kept mounted: a WebGPU surface is tied
// to its canvas). Scene = the open scene through the editor camera; Game = Play mode.
//   right drag / Alt+left drag: orbit · middle drag: pan · wheel: zoom · F: frame selection
//   click: select · Ctrl+S: save scene · Play mode: keyboard = 3DS buttons (K/J/I/U, WASD, arrows, Enter)
import { useEffect, useRef, useState } from 'react';
import { frameSelected } from '../selection';
import { getEngine, saveScene, startEngine, startPlay, stopPlay } from '../engine';
import { getState, log, setState, useStore } from '../store';

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
  const [gizmo, setGizmo] = useState<'move' | 'rotate' | 'scale'>('move');
  const setMode = (m: 'move' | 'rotate' | 'scale') => {
    setGizmo(m);
    getEngine()?.set_gizmo_mode(m);
  };

  useEffect(() => {
    const c = canvas.current!;
    let raf = 0;
    let last = performance.now();
    let frames = 0;
    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = Math.max(1, Math.round(c.clientWidth * dpr));
      const h = Math.max(1, Math.round(c.clientHeight * dpr));
      if (c.width !== w || c.height !== h) {
        c.width = w;
        c.height = h;
        getEngine()?.resize(w, h);
      }
    };
    const ro = new ResizeObserver(resize);
    ro.observe(c);
    resize();
    startEngine(c)
      .then(() => {
        resize();
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
      if (ev.code === 'KeyF') frameSelected();
      if (ev.code === 'KeyW') setMode('move');
      if (ev.code === 'KeyE') setMode('rotate');
      if (ev.code === 'KeyR') setMode('scale');
      if (ev.code === 'Delete' && getState().entity) {
        e.delete_entity(getState().entity!);
        setState((s) => ({ entity: null, sceneDirty: true, hierarchyRev: s.hierarchyRev + 1 }));
      }
      if ((ev.ctrlKey || ev.metaKey) && ev.code === 'KeyS') {
        ev.preventDefault();
        void saveScene();
      }
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
    const onGizmo = ev.button === 0 && !ev.altKey && e.gizmo_begin((ev.clientX - r.left) / r.width, (ev.clientY - r.top) / r.height, r.width, r.height);
    const mode = onGizmo ? 'gizmo' : ev.button === 2 || (ev.button === 0 && ev.altKey) ? 'orbit' : ev.button === 1 ? 'pan' : 'click';
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
      e.gizmo_drag(dx, dy, r.width, r.height);
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
      if (d.moved) setState((s) => ({ sceneDirty: true, hierarchyRev: s.hierarchyRev + 1 }));
      return;
    }
    if (!d || !e || d.mode !== 'click' || d.moved) return;
    const r = canvas.current!.getBoundingClientRect();
    const id = e.pick((ev.clientX - r.left) / r.width, (ev.clientY - r.top) / r.height) ?? null;
    setState({ entity: id });
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
        <div className="seg" title="Gizmo: W move · E rotate · R scale">
          {(['move', 'rotate', 'scale'] as const).map((m) => (
            <button key={m} className={gizmo === m ? 'on' : ''} onClick={() => setMode(m)}>
              {m === 'move' ? '✥' : m === 'rotate' ? '⟳' : '⤢'}
            </button>
          ))}
        </div>
        <span className="dim scene-name">
          {scene ? `${scene}${dirty ? ' *' : ''}` : 'no scene open — double-click a .scene.json'}
        </span>
        <div className="spacer" />
        <label className="dim check">
          <input type="checkbox" checked={dual} onChange={(e) => setState({ dual3ds: e.target.checked })} disabled={playing} /> 3DS screens
        </label>
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
