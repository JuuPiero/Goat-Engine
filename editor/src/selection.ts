// Scene selection helpers shared by the Viewport, Hierarchy and Inspector.
import { getEngine } from './engine';
import { setState } from './store';

export function selectEntity(id: string | null) {
  getEngine()?.select(id ?? undefined);
  setState({ entity: id });
}

export function frameSelected() {
  getEngine()?.frame_selected();
}
