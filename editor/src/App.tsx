// The editor window: a toolbar over dockable panels (dockview): drag tabs to rearrange, split,
// stack, float; the layout is remembered. Default = Unity's: Hierarchy | Scene/Game | Inspector
// over Project + Console.
import { useEffect, useRef } from 'react';
import { DockviewReact, themeDark, type DockviewApi, type DockviewReadyEvent, type IDockviewPanelProps } from 'dockview-react';
import 'dockview-react/dist/styles/dockview.css';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { listen } from '@tauri-apps/api/event';
import { api } from './api';
import { handleDrop, installDropHandling } from './dnd';
import { getState, log, refresh, run, setState, useStore } from './store';
import { AssetPanel } from './panels/AssetPanel';
import { Console, Hierarchy, Inspector } from './panels/Panels';
import { Viewport } from './panels/Viewport';
import { AnimatorWindow } from './panels/AnimatorWindow';
import { getEngine, openScene, startPlay, stopPlay } from './engine';

const LAYOUT_KEY = 'goat.editor.layout.v1';

const components: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  viewport: () => <Viewport />,
  hierarchy: () => <div className="panel-scroll hierarchy-panel"><Hierarchy /></div>,
  inspector: () => <div className="panel-scroll"><Inspector /></div>,
  project: () => <AssetPanel />,
  console: () => <div className="panel-scroll"><Console /></div>,
  animator: () => <AnimatorWindow />,
};

function defaultLayout(dv: DockviewApi) {
  dv.clear();
  dv.addPanel({ id: 'viewport', component: 'viewport', title: 'Scene', renderer: 'always' });
  dv.addPanel({ id: 'hierarchy', component: 'hierarchy', title: 'Hierarchy', position: { referencePanel: 'viewport', direction: 'left' }, initialWidth: 260 });
  dv.addPanel({ id: 'inspector', component: 'inspector', title: 'Inspector', position: { referencePanel: 'viewport', direction: 'right' }, initialWidth: 330 });
  dv.addPanel({ id: 'project', component: 'project', title: 'Project', position: { direction: 'below' }, initialHeight: 300 });
  dv.addPanel({ id: 'console', component: 'console', title: 'Console', position: { referencePanel: 'project', direction: 'within' } });
  dv.getPanel('project')?.api.setActive();
  dv.getPanel('viewport')?.api.setActive();
}

export function App() {
  const project = useStore((s) => s.project);
  const playing = useStore((s) => s.playing);
  const dv = useRef<DockviewApi | null>(null);
  const controller = useStore((s) => s.controller);
  // opening a controller shows the Animator window (next to the Scene view, like Unity)
  useEffect(() => {
    const d = dv.current;
    if (!controller || !d) return;
    const p = d.getPanel('animator') ?? d.addPanel({ id: 'animator', component: 'animator', title: 'Animator', position: { referencePanel: 'viewport', direction: 'within' } });
    p.api.setActive();
  }, [controller]);

  useEffect(() => {
    void run('project', async () => {
      const p = await api.projectInfo();
      setState({ project: p });
      log(`project ${p.name} — ${p.assets}`);
    });
    void installDropHandling();
    const un = listen('assets-changed', () => refresh());
    // automation / tests: window.__goat
    (window as unknown as { __goat: unknown }).__goat = { getState, setState, handleDrop, api, engine: getEngine, dock: () => dv.current, openScene, startPlay, stopPlay };
    return () => void un.then((f) => f());
  }, []);

  const onReady = (e: DockviewReadyEvent) => {
    dv.current = e.api;
    let restored = false;
    try {
      const saved = localStorage.getItem(LAYOUT_KEY);
      if (saved) {
        e.api.fromJSON(JSON.parse(saved));
        restored = !!e.api.getPanel('viewport');
      }
    } catch {
      restored = false;
    }
    if (!restored) defaultLayout(e.api);
    e.api.onDidLayoutChange(() => {
      try {
        localStorage.setItem(LAYOUT_KEY, JSON.stringify(e.api.toJSON()));
      } catch {
        /* storage unavailable: the layout just isn't remembered */
      }
    });
  };

  const resetLayout = () => {
    try {
      localStorage.removeItem(LAYOUT_KEY);
    } catch {
      /* ignore */
    }
    if (dv.current) defaultLayout(dv.current);
  };

  const newWindow = () => {
    const w = new WebviewWindow(`editor-${Date.now()}`, { url: '/', title: 'GoatEngine Editor', width: 1200, height: 800 });
    w.once('tauri://error', (e) => log(`new window: ${JSON.stringify(e.payload)}`, 'error'));
  };

  return (
    <div className="app">
      <div className="toolbar">
        <span className="brand">
          <span className="logo">G</span> GoatEngine
        </span>
        <span className="project-name">{project?.name}</span>
        <div className="spacer" />
        <div className="play-group">
          <button className={`play${playing ? ' on' : ''}`} onClick={() => (playing ? stopPlay() : void startPlay())} title="Play / Stop">
            {playing ? '■' : '▶'}
          </button>
        </div>
        <div className="spacer" />
        <button onClick={resetLayout} title="Back to the default panel layout">
          Reset Layout
        </button>
        <button onClick={newWindow} title="A second editor window (drag assets between them)">
          New Window
        </button>
      </div>
      <DockviewReact className="dock" theme={themeDark} components={components} onReady={onReady} />
    </div>
  );
}
