// Drag & drop of assets — one mechanism for every case, like a file manager:
//
// - Dragging assets out of the Project panel starts a *native* OS drag of their files
//   (tauri-plugin-drag → OLE DoDragDrop on Windows). So the same drag can be dropped on a
//   folder of this panel, in another editor window or instance, in Explorer, a chat app, an
//   image editor...
// - Drops arriving in an editor window (from itself, another editor, or the OS) come through
//   Tauri's drag-drop events with file paths + position. The element under the position with a
//   `data-drop` attribute decides what happens:
//     folder:<key>   files of this project → move there (Ctrl held → copy); outside files → import (copy)
//     scene / hierarchy  instantiate (models / prefabs) — reported until the scene view lands
import { startDrag } from '@crabnebula/tauri-plugin-drag';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { api, absPath, nameOf } from './api';
import { getState, log, refresh, run, setState } from './store';
import { getEngine, syncFiles } from './engine';

let dragIcon = '';
let ctrlDown = false;
/// Keys this window is dragging right now (the drag can come back to us).
let outgoing: string[] | null = null;

export function draggingOut(): string[] | null {
  return outgoing;
}

/// Begins a native drag of `keys` (call from a pointermove with the button held).
export function beginAssetDrag(keys: string[]) {
  const p = getState().project;
  if (!p || keys.length === 0 || outgoing) return;
  outgoing = keys;
  const paths = keys.map((k) => absPath(p.assets, k));
  startDrag({ item: paths, icon: dragIcon })
    .catch((e) => log(`drag: ${e}`, 'error'))
    .finally(() => {
      outgoing = null;
    });
}

function dropTargetAt(x: number, y: number): string | null {
  const el = document.elementFromPoint(x, y)?.closest('[data-drop]') as HTMLElement | null;
  return el?.dataset.drop ?? null;
}

/// What a drop of `paths` on `target` does (also the test hook).
export async function handleDrop(paths: string[], target: string | null, copy = ctrlDown, at?: [number, number]) {
  if (!target || paths.length === 0) return;
  if (target.startsWith('folder:')) {
    const dest = target.slice('folder:'.length);
    const keys = await api.keysOf(paths);
    const inside = keys.filter((k): k is string => k !== null);
    const outside = paths.filter((_, i) => keys[i] === null);
    if (inside.length) {
      if (copy) {
        const made = await run('copy', () => api.copyPaths(inside.map((k) => absPath(getState().project!.assets, k)), dest));
        if (made) log(`copied ${made.join(', ')}`);
      } else {
        const moved = await run('move', () => api.move(inside, dest));
        if (moved) {
          const changed = moved.filter((k, i) => k !== inside[i]);
          if (changed.length) log(`moved ${inside.filter((k, i) => k !== moved[i]).join(', ')} → ${dest || 'Assets'}`);
          setState({ selection: moved });
        }
      }
    }
    if (outside.length) {
      const made = await run('import', () => api.copyPaths(outside, dest));
      if (made) {
        log(`imported ${made.join(', ')}`);
        setState({ selection: made });
      }
    }
    refresh();
  } else if (target === 'scene' || target === 'hierarchy') {
    const keys = (await api.keysOf(paths)).filter((k): k is string => k !== null);
    const e = getEngine();
    if (!e || !getState().scene) {
      log('open a scene first (double-click a .scene.json)', 'warn');
      return;
    }
    await syncFiles(e);
    for (const key of keys) {
      try {
        const id = e.instantiate(key, at?.[0] ?? 0.5, at?.[1] ?? 0.5);
        setState((s) => ({ entity: id, sceneDirty: true, hierarchyRev: s.hierarchyRev + 1 }));
        e.select(id);
        log(`added ${nameOf(key)}`);
      } catch (err) {
        log(`${key}: ${err}`, 'error');
      }
    }
  }
}

/// Wires the window's drag-drop events (once, at startup).
export async function installDropHandling() {
  dragIcon = await api.dragIcon().catch(() => '');
  window.addEventListener('keydown', (e) => (ctrlDown = e.ctrlKey));
  window.addEventListener('keyup', (e) => (ctrlDown = e.ctrlKey));
  await getCurrentWebview().onDragDropEvent((ev) => {
    const p = ev.payload;
    const scale = window.devicePixelRatio || 1;
    if (p.type === 'enter' || p.type === 'over') {
      const t = dropTargetAt(p.position.x / scale, p.position.y / scale);
      if (t !== getState().dropTarget) setState({ dropTarget: t });
    } else if (p.type === 'drop') {
      const [x, y] = [p.position.x / scale, p.position.y / scale];
      const t = dropTargetAt(x, y);
      setState({ dropTarget: null });
      // a point of the scene view (0..1) for instantiation
      const c = document.querySelector('.viewport-canvas canvas')?.getBoundingClientRect();
      const at: [number, number] | undefined = c && c.width > 0 ? [(x - c.left) / c.width, (y - c.top) / c.height] : undefined;
      void handleDrop(p.paths, t, ctrlDown, at);
    } else {
      setState({ dropTarget: null });
    }
  });
}
