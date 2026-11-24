// Editor state shared by panels: a tiny store (subscribe + snapshot) used through `useStore`.
import { useSyncExternalStore } from 'react';
import type { ProjectInfo } from './api';
import type { MenuItem } from './ui/Menu';

export interface LogLine {
  time: string;
  level: 'info' | 'warn' | 'error';
  text: string;
}

export interface EditorState {
  project: ProjectInfo | null;
  /// Folder shown in the Project panel.
  dir: string;
  /// Selected asset keys (Project panel), in click order; the last one is active.
  selection: string[];
  /// `data-drop` value of the element under an incoming drag.
  dropTarget: string | null;
  logs: LogLine[];
  /// Bumped when files change (re-list).
  revision: number;
  clipboard: { keys: string[]; cut: boolean } | null;
  /// Scene open in the Scene view.
  scene: string | null;
  sceneDirty: boolean;
  /// Selected entity of the open scene (engine id): the active one of `entities`.
  entity: string | null;
  /// Every selected entity (Hierarchy multi-selection), active last.
  entities: string[];
  /// Entity whose name is being edited in the Hierarchy (F2).
  renaming: string | null;
  /// Undo / Redo labels for the Edit menu.
  undoLabel: string | null;
  redoLabel: string | null;
  contextMenu: { x: number; y: number; items: MenuItem[] } | null;
  /// Bumped when the scene's entities change (Hierarchy re-reads the engine).
  hierarchyRev: number;
  playing: boolean;
  dual3ds: boolean;
  viewTab: 'scene' | 'game';
  /// The controller open in the Animator window.
  controller: string | null;
}

let state: EditorState = { project: null, dir: '', selection: [], dropTarget: null, logs: [], revision: 0, clipboard: null, scene: null, sceneDirty: false, entity: null, entities: [], renaming: null, undoLabel: null, redoLabel: null, contextMenu: null, hierarchyRev: 0, playing: false, dual3ds: true, viewTab: 'scene', controller: null };
const listeners = new Set<() => void>();

export function getState(): EditorState {
  return state;
}

export function setState(patch: Partial<EditorState> | ((s: EditorState) => Partial<EditorState>)) {
  const p = typeof patch === 'function' ? patch(state) : patch;
  state = { ...state, ...p };
  listeners.forEach((l) => l());
}

export function useStore<T>(select: (s: EditorState) => T): T {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => select(state),
  );
}

export function log(text: string, level: LogLine['level'] = 'info') {
  const time = new Date().toLocaleTimeString();
  setState((s) => ({ logs: [...s.logs.slice(-499), { time, level, text }] }));
  if (level === 'error') console.error(text);
}

export function refresh() {
  setState((s) => ({ revision: s.revision + 1 }));
}

/// Runs an editor action; errors go to the Console instead of being lost.
export async function run<T>(label: string, f: () => Promise<T>): Promise<T | undefined> {
  try {
    return await f();
  } catch (e) {
    log(`${label}: ${e}`, 'error');
    return undefined;
  }
}
