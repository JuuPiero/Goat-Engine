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
import { getEngine, openScene, saveScene, startPlay, stepFrame, stopPlay, togglePause } from './engine';
import { ProfilerPanel } from './panels/ProfilerPanel';
import { BuildSettingsWindow, ProjectSettingsWindow } from './panels/SettingsWindows';
import { ContextMenuHost, MenuBar, type MenuItem } from './ui/Menu';
import * as commands from './commands';
import { copy, cut, deleteSelection, duplicate, gameObjectMenu, hasClipboard, installShortcuts, mutate, paste, redo, selectAll, undo } from './commands';
import { frameSelected } from './selection';
import { componentInfo, displayName } from './panels/ComponentEditor';

const LAYOUT_KEY = 'goat.editor.layout.v1';

const components: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  viewport: () => <Viewport />,
  hierarchy: () => <div className="panel-scroll hierarchy-panel"><Hierarchy /></div>,
  inspector: () => <div className="panel-scroll"><Inspector /></div>,
  project: () => <AssetPanel />,
  console: () => <div className="panel-scroll"><Console /></div>,
  animator: () => <AnimatorWindow />,
  profiler: () => <ProfilerPanel />,
  build: () => <div className="panel-scroll"><BuildSettingsWindow /></div>,
  settings: () => <ProjectSettingsWindow />,
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
  const paused = useStore((s) => s.paused);
  const dv = useRef<DockviewApi | null>(null);
  const controller = useStore((s) => s.controller);
  const revision = useStore((s) => s.revision);
  useEffect(() => {
    if (!project) return;
    void api.search('.scene.json').then((list) => (sceneFiles = list.map((e) => e.key).filter((k) => k.endsWith('.scene.json')).sort()));
  }, [project, revision]);
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
    const offKeys = installShortcuts();
    const windowKeys = (ev: KeyboardEvent) => {
      if ((ev.ctrlKey || ev.metaKey) && ev.shiftKey && ev.code === 'KeyB') {
        ev.preventDefault();
        showPanel(dv.current, 'build', 'Build Settings');
      }
    };
    window.addEventListener('keydown', windowKeys);
    const un = listen('assets-changed', () => refresh());
    // C# translation results (tools/goat-cs, run by the backend when .cs files change)
    void listen<{ level: 'info' | 'warn' | 'error'; text: string }>('script-log', (ev) => log(ev.payload.text, ev.payload.level));
    // automation / tests: window.__goat
    (window as unknown as { __goat: unknown }).__goat = { getState, setState, handleDrop, api, engine: getEngine, dock: () => dv.current, openScene, startPlay, stopPlay, commands };
    return () => {
      offKeys();
      window.removeEventListener('keydown', windowKeys);
      void un.then((f) => f());
    };
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
          <span className="logo">G</span>
        </span>
        <MenuBar menus={menus(dv, resetLayout, newWindow)} />
        <span className="project-name">{project?.name}</span>
        <div className="spacer" />
        <div className="play-group">
          <button className={`play${playing ? ' on' : ''}`} onClick={() => (playing ? stopPlay() : void startPlay())} title="Play / Stop (Ctrl+P)">
            {playing ? '■' : '▶'}
          </button>
          <button className={`play${paused ? ' on' : ''}`} disabled={!playing} onClick={togglePause} title="Pause (Ctrl+Shift+P)">
            ❚❚
          </button>
          <button className="play" disabled={!playing} onClick={stepFrame} title="Step (Ctrl+Alt+P)">
            ▶❚
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
      <ContextMenuHost />
    </div>
  );
}

/// Scene files of the project (File ▸ Open Scene), refreshed when files change.
let sceneFiles: string[] = [];

const PANELS: { id: string; title: string; key?: string }[] = [
  { id: 'viewport', title: 'Scene', key: 'Ctrl+1' },
  { id: 'hierarchy', title: 'Hierarchy', key: 'Ctrl+4' },
  { id: 'inspector', title: 'Inspector', key: 'Ctrl+3' },
  { id: 'project', title: 'Project', key: 'Ctrl+5' },
  { id: 'console', title: 'Console', key: 'Ctrl+Shift+C' },
  { id: 'animator', title: 'Animator' },
  { id: 'profiler', title: 'Profiler', key: 'Ctrl+7' },
];

/// Opens (or focuses) a panel of the dock.
function showPanel(d: DockviewApi | null, id: string, title: string) {
  if (!d) return;
  const p = d.getPanel(id) ?? d.addPanel({ id, component: id, title, position: { referencePanel: 'viewport', direction: 'within' } });
  p.api.setActive();
}

/// Unity's main menu: File, Edit, Assets, GameObject, Component, Window, Help.
function menus(dv: React.RefObject<DockviewApi | null>, resetLayout: () => void, newWindow: () => void): { title: string; items: () => MenuItem[] }[] {
  const s = getState;
  const noScene = () => !s().scene || s().playing;
  return [
    {
      title: 'File',
      items: () => [
        {
          label: 'New Scene',
          action: () =>
            void run('new scene', async () => {
              const key = await api.createAsset(s().dir, 'scene');
              await openScene(key);
            }),
        },
        {
          label: 'Open Scene',
          children: sceneFiles.length ? sceneFiles.map((k) => ({ label: k, checked: k === s().scene, action: () => void openScene(k) })) : [{ label: '(no scenes)', disabled: true }],
        },
        { label: 'Save', shortcut: 'Ctrl+S', disabled: noScene(), action: () => void saveScene() },
        '-',
        { label: 'Build Settings…', shortcut: 'Ctrl+Shift+B', action: () => showPanel(dv.current, 'build', 'Build Settings') },
        '-',
        { label: 'New Window', action: newWindow },
      ],
    },
    {
      title: 'Edit',
      items: () => [
        { label: s().undoLabel ? `Undo ${s().undoLabel}` : 'Undo', shortcut: 'Ctrl+Z', disabled: !s().undoLabel || s().playing, action: undo },
        { label: s().redoLabel ? `Redo ${s().redoLabel}` : 'Redo', shortcut: 'Ctrl+Y', disabled: !s().redoLabel || s().playing, action: redo },
        '-',
        { label: 'Cut', shortcut: 'Ctrl+X', disabled: !s().entities.length, action: cut },
        { label: 'Copy', shortcut: 'Ctrl+C', disabled: !s().entities.length, action: copy },
        { label: 'Paste', shortcut: 'Ctrl+V', disabled: !hasClipboard() || noScene(), action: () => paste(null) },
        { label: 'Duplicate', shortcut: 'Ctrl+D', disabled: !s().entities.length, action: duplicate },
        { label: 'Delete', shortcut: 'Del', disabled: !s().entities.length, action: () => deleteSelection() },
        '-',
        { label: 'Select All', shortcut: 'Ctrl+A', disabled: noScene(), action: selectAll },
        { label: 'Frame Selected', shortcut: 'F', disabled: !s().entities.length, action: frameSelected },
        '-',
        { label: s().playing ? 'Stop' : 'Play', shortcut: 'Ctrl+P', action: () => (s().playing ? stopPlay() : void startPlay()) },
        { label: 'Pause', shortcut: 'Ctrl+Shift+P', checked: s().paused, disabled: !s().playing, action: togglePause },
        { label: 'Step', shortcut: 'Ctrl+Alt+P', disabled: !s().playing, action: stepFrame },
        '-',
        { label: 'Project Settings…', action: () => showPanel(dv.current, 'settings', 'Project Settings') },
      ],
    },
    {
      title: 'Assets',
      items: () => [
        {
          label: 'Create',
          children: (['scene', 'prefab', 'material', 'controller'] as const).map((k) => ({
            label: { scene: 'Scene', prefab: 'Prefab', material: 'Material', controller: 'Animator Controller' }[k],
            action: () =>
              void run('create', async () => {
                const key = await api.createAsset(s().dir, k);
                setState({ selection: [key] });
                refresh();
              }),
          })),
        },
        { label: 'Show in Explorer', disabled: !s().selection.length, action: () => void api.reveal(s().selection[s().selection.length - 1]) },
        { label: 'Refresh', shortcut: 'Ctrl+R', action: refresh },
      ],
    },
    { title: 'GameObject', items: () => (noScene() ? [{ label: 'Open a scene first', disabled: true }] : gameObjectMenu(() => s().entity)) },
    {
      title: 'Component',
      items: () => {
        const id = s().entity;
        if (!id || noScene()) return [{ label: 'Select a GameObject first', disabled: true }];
        const info = componentInfo().components;
        const present = new Set((JSON.parse(getEngine()?.components(id) ?? '[]') as { name: string }[]).map((c) => c.name));
        const groups = new Map<string, MenuItem[]>();
        for (const [name, i] of Object.entries(info)) {
          if (!i.menu) continue;
          const [group] = i.menu.split('/');
          const add = () =>
            mutate(`Add ${displayName(name)}`, () => {
              for (const n of [...(i.requires ?? []), name]) if (!present.has(n)) getEngine()?.set_component(id, n, JSON.stringify(info[n]?.defaults ?? {}));
            });
          groups.set(group, [...(groups.get(group) ?? []), { label: i.menu.split('/').slice(1).join(' / '), disabled: present.has(name), action: add }]);
        }
        return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([g, children]) => ({ label: g, children }));
      },
    },
    {
      title: 'Window',
      items: () => [...PANELS.map((p) => ({ label: p.title, shortcut: p.key, action: () => showPanel(dv.current, p.id, p.title) })), '-', { label: 'Reset Layout', action: resetLayout }],
    },
    { title: 'Help', items: () => [{ label: 'GoatEngine — Unity-like engine for the 3DS and desktop', disabled: true }] },
  ];
}