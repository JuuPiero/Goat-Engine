// Scene view and Game view panels (Unity's / PiaEngine's), each with its own canvas (viewport.ts):
// both draw every frame, side by side or as tabs.
//
// Scene view: bar (2D, Grid, Gizmos, renderer badge, hints), the tools overlay (Q W E R), the view
// gizmo (click an axis: look along it; the label under it toggles Persp / Iso), editing by mouse:
//   right drag / Alt+left drag: orbit (pan in 2D) · middle drag or the Hand tool: pan · wheel: zoom
//   F: frame · click: select (Ctrl / Shift: add / remove) · handles: move / rotate / scale (Ctrl snaps)
// While playing it shows the running game through the editor camera, with the grid, gizmos,
// picking and handles (Unity: Play-mode edits of the running objects, gone on Stop).
// Game view: the game's cameras (also in edit mode), size menu (3DS screens, aspects,
// resolutions), Scale, Stats; keyboard = 3DS buttons while playing.
import { useEffect, useRef, useState } from 'react';
import type { DockviewPanelApi } from 'dockview-react';
import { Box, ChevronLeft, Grid3x3, Hand, Move, Move3d, Rotate3d, Scale3d, Shapes } from 'lucide-react';
import { frameSelected, selectEntities } from '../selection';
import { afterChange } from '../commands';
import { exitPrefab, getEngine, saveScene, startPlay, stopPlay } from '../engine';
import { getState, log, setState, useStore } from '../store';
import { afterFrame, attach, GAME_SIZES, relayout, setVisible, viewMode, type ViewMode } from '../viewport';

/// The view's own canvas in this panel; the engine draws it while the panel is on screen.
function useCanvasHost(api: DockviewPanelApi, m: ViewMode) {
  const host = useRef<HTMLDivElement>(null);
  const [owner, setOwner] = useState(false);
  useEffect(() => {
    const show = (on: boolean) => {
      if (on && host.current) attach(host.current, m);
      else setVisible(m, false);
      setOwner(on);
    };
    show(api.isVisible);
    const offs = [api.onDidVisibilityChange((e) => show(e.isVisible))];
    return () => {
      offs.forEach((d) => d.dispose());
      setVisible(m, false);
    };
  }, [api, m]);
  return { host, owner };
}

const TOOLS = [
  { id: 'hand', icon: Hand, title: 'View Tool (Q)' },
  { id: 'move', icon: Move, title: 'Move Tool (W)' },
  { id: 'rotate', icon: Rotate3d, title: 'Rotate Tool (E)' },
  { id: 'scale', icon: Scale3d, title: 'Scale Tool (R)' },
  { id: 'transform', icon: Move3d, title: 'Transform Tool (Y)' },
] as const;

const UNDO_LABEL: Record<string, string> = { move: 'Move', rotate: 'Rotate', scale: 'Scale', transform: 'Transform' };

const typing = () => ['INPUT', 'TEXTAREA', 'SELECT'].includes((document.activeElement?.tagName ?? '').toUpperCase());

export function SceneView({ api }: { api: DockviewPanelApi }) {
  const { host, owner } = useCanvasHost(api, 'scene');
  const playing = useStore((s) => s.playing);
  const scene = useStore((s) => s.scene);
  const dirty = useStore((s) => s.sceneDirty);
  const drop = useStore((s) => s.dropTarget);
  const tool = useStore((s) => s.tool);
  const view2d = useStore((s) => s.view2d);
  const grid = useStore((s) => s.showGrid);
  const gizmos = useStore((s) => s.showGizmos);
  const error = useStore((s) => s.engineError);
  const ready = useStore((s) => s.engineReady);
  const prefabStack = useStore((s) => s.prefabStack);
  const [backend, setBackend] = useState('');
  const drag = useRef<{ x: number; y: number; mode: 'orbit' | 'pan' | 'click' | 'gizmo'; moved: boolean } | null>(null);
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const lastLive = useRef(0);
  useEffect(() => {
    if (ready) setBackend(getEngine()?.backend_name() ?? '');
  }, [ready]);

  // keyboard (pointer over the Scene view): Q W E R tools, Z / X handles, F frame
  useEffect(() => {
    const down = (ev: KeyboardEvent) => {
      if (typing() || !host.current?.matches(':hover')) return;
      if (ev.ctrlKey || ev.metaKey || ev.altKey) return;
      const s = getState();
      const k = ev.code;
      if (k === 'KeyQ') setState({ tool: 'hand' });
      else if (k === 'KeyW') setState({ tool: 'move' });
      else if (k === 'KeyE') setState({ tool: 'rotate' });
      else if (k === 'KeyR') setState({ tool: 'scale' });
      else if (k === 'KeyY') setState({ tool: 'transform' });
      else if (k === 'KeyZ') setState({ pivotCenter: !s.pivotCenter });
      else if (k === 'KeyX') setState({ toolLocal: !s.toolLocal });
      else if (k === 'KeyF') frameSelected();
      else return;
      ev.preventDefault();
    };
    window.addEventListener('keydown', down);
    return () => window.removeEventListener('keydown', down);
  }, []);

  const set2d = (on: boolean) => {
    getEngine()?.set_2d(on);
    setState({ view2d: on });
  };

  const onPointerDown = (ev: React.PointerEvent) => {
    const e = getEngine();
    if (!e || !owner) return;
    const r = (ev.currentTarget as HTMLElement).querySelector('canvas')!.getBoundingClientRect();
    // Play mode too (Unity): the handles move the running objects (changes are gone on Stop)
    const onGizmo = ev.button === 0 && !ev.altKey && tool !== 'hand' && e.gizmo_begin((ev.clientX - r.left) / r.width, (ev.clientY - r.top) / r.height, r.width, r.height);
    // the drag is one Undo step (recorded before the first change)
    if (onGizmo && !e.is_playing()) e.record_undo(UNDO_LABEL[tool] ?? 'Move');
    const mode = onGizmo ? 'gizmo' : ev.button === 2 || (ev.button === 0 && ev.altKey) ? 'orbit' : ev.button === 1 || (ev.button === 0 && tool === 'hand') ? 'pan' : 'click';
    drag.current = { x: ev.clientX, y: ev.clientY, mode, moved: false };
    (ev.currentTarget as HTMLElement).setPointerCapture(ev.pointerId);
    ev.preventDefault();
  };
  const onPointerMove = (ev: React.PointerEvent) => {
    const d = drag.current;
    const e = getEngine();
    const r = (ev.currentTarget as HTMLElement).querySelector('canvas')?.getBoundingClientRect();
    if (r) pointer.current = { x: ev.clientX - r.left, y: ev.clientY - r.top };
    // no button: highlight the handle under the pointer (like three.js TransformControls)
    if (!d && e && r && owner && tool !== 'hand') e.gizmo_hover((ev.clientX - r.left) / r.width, (ev.clientY - r.top) / r.height, r.width, r.height);
    if (!d || !e) return;
    const dx = ev.clientX - d.x;
    const dy = ev.clientY - d.y;
    if (Math.abs(dx) + Math.abs(dy) > 2) d.moved = true;
    if (d.mode === 'orbit') e.orbit(dx, dy);
    if (d.mode === 'pan') e.pan(dx, dy);
    if (d.mode === 'gizmo') {
      const r = (ev.currentTarget as HTMLElement).querySelector('canvas')!.getBoundingClientRect();
      e.gizmo_drag(dx, dy, r.width, r.height, ev.ctrlKey || ev.metaKey);
      // the Inspector (Transform numbers) follows the drag live, like Unity (~30 times a second)
      const now = performance.now();
      if (now - lastLive.current > 33) {
        lastLive.current = now;
        setState((s) => ({ hierarchyRev: s.hierarchyRev + 1 }));
      }
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
    const r = (ev.currentTarget as HTMLElement).querySelector('canvas')!.getBoundingClientRect();
    const id = e.pick((ev.clientX - r.left) / r.width, (ev.clientY - r.top) / r.height) ?? null;
    // Ctrl / Shift + click: add to / remove from the selection (Unity)
    if ((ev.ctrlKey || ev.shiftKey || ev.metaKey) && id) {
      const cur = getState().entities;
      selectEntities(cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]);
    } else selectEntities(id ? [id] : []);
  };

  return (
    <div className="g-panel g-viewpanel">
      <div className="g-viewbar">
        <button className={`g-tbtn${view2d ? ' on' : ''}`} title="2D view (orthographic, X right / Y up)" onClick={() => set2d(!view2d)}>
          2D
        </button>
        <button className={`g-tbtn${grid ? ' on' : ''}`} title="Show the grid" onClick={() => setState({ showGrid: !grid })}>
          <Grid3x3 size={13} /> Grid
        </button>
        <button className={`g-tbtn${gizmos ? ' on' : ''}`} title="Show component gizmos (colliders, cameras, lights…)" onClick={() => setState({ showGizmos: !gizmos })}>
          <Shapes size={13} /> Gizmos
        </button>
        <span className="g-pipeline-badge" title="Renderer of the editor">
          3D · {backend ? backend.split(' ')[0] : 'wgpu'}
        </span>
        <span className="g-viewbar-label g-ellipsis">{scene ? `${scene}${dirty ? ' *' : ''}` : 'no scene — double-click a .scene.json'}</span>
        <span className="g-flex" />
        <span className="g-viewbar-hint">{view2d ? 'RMB / MMB pan · Wheel zoom · F frame' : 'RMB orbit · MMB pan · Wheel zoom · F frame'}</span>
        <button className="g-tbtn" disabled={!scene || playing} onClick={() => void saveScene()} title="Save scene (Ctrl+S)">
          Save
        </button>
      </div>
      {prefabStack.length > 0 && scene && (
        <div className="g-prefab-crumbs">
          <button className="g-tbtn" title="Back to the scene (saves the prefab)" onClick={() => void exitPrefab()}>
            <ChevronLeft size={13} /> Back
          </button>
          <span className="g-status-dim">{prefabStack.map((k) => k.split('/').pop()?.replace(/\.(scene|prefab)\.json$/, '')).join(' › ')} ›</span>
          <span className="prefab">{scene.split('/').pop()?.replace('.prefab.json', '')}</span>
          <span className="g-status-dim">(Prefab Mode · Auto Save)</span>
        </div>
      )}
      <div className="g-viewport-wrap">
        <div
          ref={host}
          className={`g-viewport${drop === 'scene' ? ' drop' : ''}`}
          data-drop="scene"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerLeave={() => {
            pointer.current = null;
            const e = getEngine();
            if (!drag.current && e) e.gizmo_hover(-1, -1, 1, 1);
          }}
          onWheel={(ev) => owner && getEngine()?.zoom(ev.deltaY)}
          onContextMenu={(e) => e.preventDefault()}
        >

          {error && <div className="g-nocamera">{error}</div>}
        </div>
        {owner && (
          <>
            <div className="g-tools-overlay">
              {TOOLS.map((t) => (
                <button key={t.id} className={`g-tools-overlay-btn${tool === t.id ? ' on' : ''}`} title={t.title} onClick={() => setState({ tool: t.id })}>
                  <t.icon size={15} strokeWidth={1.75} />
                </button>
              ))}
            </div>
            <SceneOverlay host={host} pointer={pointer} />
            <ViewGizmo view2d={view2d} />
          </>
        )}
      </div>
    </div>
  );
}

type OverlayItem = { k: 'l' | 's' | 'p' | 'c' | 't' | 'i'; p: number[]; s?: string | null; w?: number; a?: number; z?: boolean; f?: string; fa?: number; t?: string; n?: string; c?: string };

/// The Scene view overlay (PiaEngine's handles, selection outlines, camera / light icons): shapes
/// from the engine (`scene_overlay`, view pixels) drawn on a canvas over the engine's canvas.
function SceneOverlay({ host, pointer }: { host: React.RefObject<HTMLDivElement | null>; pointer: React.RefObject<{ x: number; y: number } | null> }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const draw = () => {
      const c = ref.current;
      const e = getEngine();
      const view = host.current?.querySelector('canvas:not(.g-overlay)') as HTMLCanvasElement | null;
      if (!c || !e || !view) return;
      const wrap = c.parentElement!.getBoundingClientRect();
      const r = view.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      Object.assign(c.style, { left: `${r.left - wrap.left}px`, top: `${r.top - wrap.top}px`, width: `${r.width}px`, height: `${r.height}px` });
      if (c.width !== Math.round(r.width * dpr) || c.height !== Math.round(r.height * dpr)) {
        c.width = Math.round(r.width * dpr);
        c.height = Math.round(r.height * dpr);
      }
      const g = c.getContext('2d')!;
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
      g.clearRect(0, 0, r.width, r.height);
      const p = pointer.current;
      let items: OverlayItem[];
      try {
        items = JSON.parse(e.scene_overlay(r.width, r.height, p?.x ?? -1, p?.y ?? -1));
      } catch {
        return;
      }
      g.lineJoin = 'round';
      g.lineCap = 'round';
      for (const it of items) {
        const q = it.p;
        g.globalAlpha = 1;
        if (it.k === 'l' || it.k === 's') {
          g.beginPath();
          if (it.k === 'l') {
            for (let i = 0; i < q.length; i += 2) (i ? g.lineTo : g.moveTo).call(g, q[i], q[i + 1]);
            if (it.z) g.closePath();
          } else {
            for (let i = 0; i + 3 < q.length; i += 4) {
              g.moveTo(q[i], q[i + 1]);
              g.lineTo(q[i + 2], q[i + 3]);
            }
          }
          g.strokeStyle = it.s ?? '#fff';
          g.lineWidth = it.w ?? 1;
          g.globalAlpha = it.a ?? 1;
          g.stroke();
        } else if (it.k === 'p') {
          g.beginPath();
          for (let i = 0; i < q.length; i += 2) (i ? g.lineTo : g.moveTo).call(g, q[i], q[i + 1]);
          g.closePath();
          g.fillStyle = it.f ?? '#fff';
          g.globalAlpha = it.fa ?? 1;
          g.fill();
          if (it.s) {
            g.globalAlpha = 1;
            g.strokeStyle = it.s;
            g.lineWidth = it.w ?? 1;
            g.stroke();
          }
        } else if (it.k === 'c') {
          g.beginPath();
          g.arc(q[0], q[1], Math.max(0, q[2]), 0, Math.PI * 2);
          if (it.f) {
            g.fillStyle = it.f;
            g.globalAlpha = it.fa ?? 1;
            g.fill();
          } else {
            g.strokeStyle = it.s ?? '#fff';
            g.lineWidth = it.w ?? 1;
            g.globalAlpha = it.a ?? 1;
            g.stroke();
          }
        } else if (it.k === 't') {
          g.font = '12px Inter, "Segoe UI", sans-serif';
          g.textAlign = 'left';
          g.textBaseline = 'top';
          g.lineWidth = 3;
          g.strokeStyle = '#000';
          g.strokeText(it.t ?? '', q[0], q[1]);
          g.fillStyle = it.c ?? '#fff';
          g.fillText(it.t ?? '', q[0], q[1]);
        } else if (it.k === 'i') {
          const [x, y] = q;
          if (it.n === 'camera2d') {
            // PiaEngine's 2D camera glyph
            g.globalAlpha = 0.85;
            g.fillStyle = '#dddddd';
            g.beginPath();
            g.roundRect(x - 9, y - 6, 13, 12, 2);
            g.fill();
            g.beginPath();
            g.moveTo(x + 4, y - 2);
            g.lineTo(x + 10, y - 6);
            g.lineTo(x + 10, y + 6);
            g.lineTo(x + 4, y + 2);
            g.closePath();
            g.fill();
          } else {
            g.font = '18px "Segoe UI Emoji", "Apple Color Emoji", sans-serif';
            g.textAlign = 'center';
            g.textBaseline = 'middle';
            g.fillText(it.n === 'camera' ? '🎥' : '💡', x, y);
          }
        }
      }
      g.globalAlpha = 1;
    };
    afterFrame.add(draw);
    draw();
    return () => void afterFrame.delete(draw);
  }, [host, pointer]);
  return <canvas ref={ref} className="g-overlay" />;
}

/// Unity's scene view gizmo: the world axes as seen by the editor camera; click a cone to look
/// along it, the label below toggles Persp / Iso.
function ViewGizmo({ view2d }: { view2d: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [ortho, setOrtho] = useState(false);
  const axes = useRef<{ id: string; x: number; y: number }[]>([]);
  useEffect(() => {
    const draw = () => {
      const c = ref.current;
      const e = getEngine();
      if (!c || !e) return;
      const st = JSON.parse(e.view_state()) as { rotation: [number, number, number, number]; view: { ortho: boolean } };
      if (st.view.ortho !== ortho) setOrtho(st.view.ortho);
      const [qx, qy, qz, qw] = st.rotation;
      // world axis → camera space: q⁻¹ · v
      // v' = v + 2w(u × v) + 2u × (u × v), with u = −(x, y, z) for the inverse rotation
      const inv = (v: [number, number, number]): [number, number, number] => {
        const [ux, uy, uz] = [-qx, -qy, -qz];
        const cx1 = uy * v[2] - uz * v[1], cy1 = uz * v[0] - ux * v[2], cz1 = ux * v[1] - uy * v[0];
        const cx2 = uy * cz1 - uz * cy1, cy2 = uz * cx1 - ux * cz1, cz2 = ux * cy1 - uy * cx1;
        return [v[0] + 2 * (qw * cx1 + cx2), v[1] + 2 * (qw * cy1 + cy2), v[2] + 2 * (qw * cz1 + cz2)];
      };
      const dpr = window.devicePixelRatio || 1;
      const S = 84;
      if (c.width !== S * dpr) {
        c.width = S * dpr;
        c.height = S * dpr;
      }
      const g = c.getContext('2d')!;
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
      g.clearRect(0, 0, S, S);
      const cx = S / 2, cy = S / 2, L = 26;
      const list = [
        { id: 'x', v: [1, 0, 0] as [number, number, number], color: '#e66b62', label: 'x' },
        { id: 'y', v: [0, 1, 0] as [number, number, number], color: '#92cf5c', label: 'y' },
        { id: 'z', v: [0, 0, 1] as [number, number, number], color: '#5c9ce6', label: 'z' },
        { id: '-x', v: [-1, 0, 0] as [number, number, number], color: '#9a9a9a' },
        { id: '-y', v: [0, -1, 0] as [number, number, number], color: '#9a9a9a' },
        { id: '-z', v: [0, 0, -1] as [number, number, number], color: '#9a9a9a' },
      ].map((a) => {
        const p = inv(a.v);
        return { ...a, x: cx + p[0] * L, y: cy - p[1] * L, depth: p[2] };
      });
      list.sort((a, b) => a.depth - b.depth);
      g.fillStyle = '#c8c8c8';
      g.beginPath();
      g.arc(cx, cy, 6, 0, Math.PI * 2);
      g.fill();
      for (const a of list) {
        if (!a.id.startsWith('-')) {
          g.strokeStyle = a.color;
          g.lineWidth = 2;
          g.beginPath();
          g.moveTo(cx, cy);
          g.lineTo(a.x, a.y);
          g.stroke();
        }
        g.fillStyle = a.color;
        g.beginPath();
        g.arc(a.x, a.y, a.id.startsWith('-') ? 5 : 7, 0, Math.PI * 2);
        g.fill();
        if (a.label) {
          g.fillStyle = '#111';
          g.font = '600 9px Inter, sans-serif';
          g.textAlign = 'center';
          g.textBaseline = 'middle';
          g.fillText(a.label, a.x, a.y + 0.5);
        }
      }
      axes.current = list;
    };
    afterFrame.add(draw);
    draw();
    return () => void afterFrame.delete(draw);
  }, [ortho]);
  if (view2d) return null;
  return (
    <div className="g-viewgizmo">
      <canvas
        ref={ref}
        className="g-axis-gizmo"
        style={{ width: 84, height: 84 }}
        onClick={(ev) => {
          const r = (ev.target as HTMLCanvasElement).getBoundingClientRect();
          const x = ev.clientX - r.left, y = ev.clientY - r.top;
          const hit = [...axes.current].reverse().find((a) => Math.hypot(a.x - x, a.y - y) < 8);
          if (hit) getEngine()?.view_axis(hit.id);
        }}
      />
      <div
        className="g-viewgizmo-label"
        title="Perspective / Isometric"
        onClick={() => {
          getEngine()?.set_ortho(!ortho);
          setOrtho(!ortho);
        }}
      >
        {ortho ? '≡ Iso' : '< Persp'}
      </div>
    </div>
  );
}

export function GameView({ api }: { api: DockviewPanelApi }) {
  const { host, owner } = useCanvasHost(api, 'game');
  const playing = useStore((s) => s.playing);
  const dual = useStore((s) => s.dual3ds);
  const size = useStore((s) => s.gameSize);
  const zoom = useStore((s) => s.gameZoom);
  const showStats = useStore((s) => s.gameStats);
  const stats = useStore((s) => s.stats);
  useEffect(() => relayout('game'), [dual, size, zoom, owner]);

  // keyboard: the 3DS buttons while playing (when the Game view holds the canvas)
  useEffect(() => {
    const down = (ev: KeyboardEvent) => {
      const e = getEngine();
      if (!e || typing() || !e.is_playing() || viewMode() !== 'game') return;
      if (ev.ctrlKey || ev.metaKey) return;
      e.key(ev.code, true);
      if (ev.code === 'Escape') stopPlay();
      ev.preventDefault();
    };
    const up = (ev: KeyboardEvent) => {
      const e = getEngine();
      if (e?.is_playing()) e.key(ev.code, false);
    };
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    return () => {
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
    };
  }, []);

  const current = dual ? '3ds' : size;
  return (
    <div className="g-panel g-viewpanel">
      <div className="g-viewbar">
        <span className="g-tbtn on">Game</span>
        <span className="g-viewbar-sep" />
        <span className="g-viewbar-label">Display 1</span>
        <select
          className="g-input g-select g-aspect"
          title="Game view size"
          value={current}
          onChange={(e) => {
            const id = e.target.value;
            if (playing && (id === '3ds') !== dual) return log('the 3DS screen layout is chosen when Play starts', 'warn');
            setState({ gameSize: id === '3ds' ? size : id, dual3ds: id === '3ds' });
          }}
        >
          {GAME_SIZES.map((g) => (
            <option key={g.id} value={g.id}>
              {g.label}
            </option>
          ))}
        </select>
        <span className="g-viewbar-label">Scale</span>
        <input type="range" className="g-scale" min={0.25} max={1} step={0.05} value={zoom} onChange={(e) => setState({ gameZoom: +e.target.value })} title="Game view zoom" />
        <span className="g-viewbar-label">{zoom.toFixed(2)}x</span>
        <span className="g-flex" />
        <button className={`g-tbtn${showStats ? ' on' : ''}`} onClick={() => setState({ gameStats: !showStats })} title="Rendering statistics">
          <Box size={13} /> Stats
        </button>
        {!playing && (
          <button className="g-tbtn" onClick={() => void startPlay()} title="Play (Ctrl+P)">
            ▶ Play
          </button>
        )}
      </div>
      <div ref={host} className={`g-viewport g-gameview${playing ? ' playing' : ''}`} tabIndex={0}>

        {owner && showStats && <div className="g-stats">{stats.replace(/ · /g, '\n')}</div>}
      </div>
    </div>
  );
}
