// Scene selection helpers shared by the Viewport, Hierarchy and Inspector.
import { getEngine } from './engine';
import { setState } from './store';

/// Selects one entity (or nothing).
export function selectEntity(id: string | null) {
  selectEntities(id ? [id] : []);
}

/// Selects several entities; the last one is active (Inspector, gizmo).
export function selectEntities(ids: string[]) {
  getEngine()?.select_many(JSON.stringify(ids));
  setState({ entities: ids, entity: ids[ids.length - 1] ?? null });
}

/// Reads the selection back from the engine (after undo, paste, duplicate…).
export function syncSelection() {
  const e = getEngine();
  if (!e) return;
  const ids: string[] = JSON.parse(e.selection());
  setState({ entities: ids, entity: ids[ids.length - 1] ?? null });
}

export function frameSelected() {
  getEngine()?.frame_selected();
}
