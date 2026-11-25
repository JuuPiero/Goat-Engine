// Project Hub (like Unity Hub / PiaEngine's): choose or create the project the editor opens.
// Backend: editor/src-tauri/src/hub.rs (recent projects in ~/.goat/hub.json, the engine's examples).
import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ArrowUp, Box, EllipsisVertical, Folder, FolderOpen, FolderSearch, HardDrive, Home, LoaderCircle, Plus, Search, Square, X } from 'lucide-react';
import './hub.css';

interface HubProject {
  path: string;
  name: string;
  dimension: '2d' | '3d' | null;
  lastOpened: number | null;
  modified: number | null;
  exists: boolean;
  example: boolean;
}

interface HubState {
  project: string | null;
  engineRoot: string | null;
  defaultLocation: string;
}

interface DirListing {
  path: string;
  parent: string | null;
  isProject: boolean;
  entries: { name: string; path: string; isProject: boolean }[];
  roots: { label: string; path: string }[];
}

const cls = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(' ');

function ago(t: number | null): string {
  if (!t) return '—';
  const s = Math.round((Date.now() - t) / 1000);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 86400 * 30) return `${Math.round(s / 86400)} days ago`;
  return new Date(t).toLocaleDateString();
}

const err = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

function DimBadge({ d }: { d: HubProject['dimension'] }) {
  return <span className={cls('hub-badge', d ?? 'none')}>{d ? d.toUpperCase() : '?'}</span>;
}

/** Opens a project: the backend switches to it, the page reloads into the editor. */
export async function openProject(path: string) {
  await invoke('hub_open', { path });
  location.hash = '';
  location.reload();
}

export function HubApp() {
  const [info, setInfo] = useState<HubState | null>(null);
  const [projects, setProjects] = useState<HubProject[] | null>(null);
  const [search, setSearch] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState<string | null>(null);
  const [dialog, setDialog] = useState<'new' | 'open' | null>(() => (location.hash === '#new' ? 'new' : location.hash === '#open' ? 'open' : null));
  const [menu, setMenu] = useState<{ x: number; y: number; p: HubProject } | null>(null);

  const reload = useCallback(() => {
    invoke<HubProject[]>('hub_projects').then(setProjects, (e) => setError(err(e)));
  }, []);
  useEffect(() => {
    invoke<HubState>('hub_state').then(setInfo, (e) => setError(err(e)));
    reload();
    if (location.hash) history.replaceState(null, '', location.pathname);
  }, [reload]);
  useEffect(() => {
    const close = () => setMenu(null);
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  }, []);

  const open = async (p: { path: string; name: string }) => {
    setError(null);
    setOpening(p.name);
    try {
      await openProject(p.path);
    } catch (e) {
      setOpening(null);
      setError(err(e));
    }
  };

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return (projects ?? []).filter((p) => !q || p.name.toLowerCase().includes(q) || p.path.toLowerCase().includes(q));
  }, [projects, search]);
  const recent = filtered.filter((p) => p.lastOpened !== null);
  const examples = filtered.filter((p) => p.lastOpened === null);

  const row = (p: HubProject) => (
    <div
      key={p.path}
      className={cls('hub-row', !p.exists && 'missing')}
      onClick={() => p.exists && void open(p)}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY, p });
      }}
      title={p.exists ? `Open ${p.path}` : 'The project folder was moved or deleted'}
    >
      <div className="hub-row-icon">{p.dimension === '2d' ? <Square size={20} /> : <Box size={20} />}</div>
      <div className="hub-row-main">
        <div className="hub-row-name">
          {p.name} <DimBadge d={p.dimension} />
          {p.example && <span className="hub-tag">Example</span>}
          {!p.exists && <span className="hub-tag warn">Missing</span>}
        </div>
        <div className="hub-row-path">{p.path}</div>
      </div>
      <div className="hub-row-time">{p.lastOpened ? ago(p.lastOpened) : 'Never opened'}</div>
      <button
        className="hub-icon-btn"
        title="More"
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          setMenu({ x: r.right - 170, y: r.bottom + 2, p });
        }}
      >
        <EllipsisVertical size={15} />
      </button>
    </div>
  );

  return (
    <div className="hub">
      <header className="hub-header">
        <div className="hub-logo">
          <svg viewBox="0 0 32 32" width="22" height="22">
            <path d="M16 2 29 9.5v13L16 30 3 22.5v-13z" fill="#d8d8d8" />
            <path d="M16 9 23 13v6l-7 4-7-4v-6z" fill="#191919" />
          </svg>
          GoatEngine Hub
        </div>
        <div className="hub-flex" />
        {info?.engineRoot && <span className="hub-dim">Engine {info.engineRoot}</span>}
      </header>
      <main className="hub-main">
        <div className="hub-toolbar">
          <h1>Projects</h1>
          <div className="hub-flex" />
          <div className="hub-search">
            <Search size={13} />
            <input placeholder="Search" value={search} onChange={(e) => setSearch(e.target.value)} />
          </div>
          <button className="g-btn hub-btn" onClick={() => setDialog('open')}>
            <FolderOpen size={14} /> Open
          </button>
          <button className="g-btn primary hub-btn" onClick={() => setDialog('new')}>
            <Plus size={14} /> New project
          </button>
        </div>
        {error && (
          <div className="hub-error">
            {error}
            <button className="hub-icon-btn" onClick={() => setError(null)}>
              <X size={13} />
            </button>
          </div>
        )}
        <div className="hub-list">
          {projects === null ? (
            <div className="hub-empty">Loading…</div>
          ) : (
            <>
              {recent.length > 0 && <div className="hub-section">Recent</div>}
              {recent.map(row)}
              {examples.length > 0 && <div className="hub-section">Examples</div>}
              {examples.map(row)}
              {!filtered.length && <div className="hub-empty">{search ? 'No project matches.' : 'No projects yet. Create one, or open a folder that contains goat.project.json.'}</div>}
            </>
          )}
        </div>
      </main>

      {menu && (
        <div className="hub-menu" style={{ left: menu.x, top: menu.y }} onPointerDown={(e) => e.stopPropagation()}>
          {menu.p.exists && (
            <div
              className="hub-menu-item"
              onClick={() => {
                setMenu(null);
                void open(menu.p);
              }}
            >
              Open
            </div>
          )}
          {menu.p.exists && (
            <div
              className="hub-menu-item"
              onClick={() => {
                setMenu(null);
                invoke('hub_reveal', { path: menu.p.path }).catch((e) => setError(err(e)));
              }}
            >
              Show in Explorer
            </div>
          )}
          {menu.p.lastOpened !== null && (
            <div
              className="hub-menu-item"
              onClick={() => {
                setMenu(null);
                invoke('hub_remove', { path: menu.p.path }).then(reload, (e) => setError(err(e)));
              }}
            >
              Remove from list
            </div>
          )}
        </div>
      )}

      {dialog === 'new' && info && (
        <NewProjectDialog
          defaultLocation={info.defaultLocation}
          onClose={() => setDialog(null)}
          onCreated={(path, name) => {
            setDialog(null);
            void open({ path, name });
          }}
        />
      )}
      {dialog === 'open' && (
        <FolderPicker
          title="Open project"
          initial={info?.defaultLocation ?? ''}
          mode="project"
          onClose={() => setDialog(null)}
          onPick={(path) => {
            setDialog(null);
            void open({ path, name: path.split('/').pop() ?? path });
          }}
        />
      )}
      {opening && (
        <div className="hub-overlay">
          <div className="hub-opening">
            <LoaderCircle size={28} className="hub-spin" />
            <div>Opening {opening}…</div>
          </div>
        </div>
      )}
    </div>
  );
}

function NewProjectDialog({ defaultLocation, onClose, onCreated }: { defaultLocation: string; onClose: () => void; onCreated: (path: string, name: string) => void }) {
  const [dimension, setDimension] = useState<'2d' | '3d'>('3d');
  const [name, setName] = useState('My project');
  const [where, setWhere] = useState(defaultLocation);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const invalid = !name.trim() || /[\\/:*?"<>|]/.test(name);
  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const path = await invoke<string>('hub_create', { name: name.trim(), location: where, dimension });
      onCreated(path, name.trim());
    } catch (e) {
      setError(err(e));
      setBusy(false);
    }
  };
  const templates = [
    { id: '3d' as const, title: '3D', desc: 'Perspective camera, a directional light, a ground and a cube. Models, lighting, physics — desktop and 3DS.', icon: <Box size={28} /> },
    { id: '2d' as const, title: '2D', desc: 'Orthographic camera and a sprite. Sprites, tilemaps, 2D physics, UI — on both 3DS screens too.', icon: <Square size={28} /> },
  ];
  return (
    <div className="hub-overlay" onPointerDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <form
        className="hub-dialog wide"
        onSubmit={(e) => {
          e.preventDefault();
          if (!invalid && where && !busy) void create();
        }}
      >
        <div className="hub-dialog-title">
          New project
          <div className="hub-flex" />
          <button type="button" className="hub-icon-btn" onClick={onClose}>
            <X size={15} />
          </button>
        </div>
        <div className="hub-templates">
          {templates.map((t) => (
            <div key={t.id} className={cls('hub-template', dimension === t.id && 'selected')} onClick={() => setDimension(t.id)}>
              <div className="hub-template-icon">{t.icon}</div>
              <div className="hub-template-title">{t.title}</div>
              <div className="hub-template-desc">{t.desc}</div>
            </div>
          ))}
        </div>
        <label className="hub-field">
          Project name
          <input className="g-input" autoFocus value={name} onChange={(e) => setName(e.target.value)} onFocus={(e) => e.target.select()} />
        </label>
        <label className="hub-field">
          Location
          <div className="hub-field-row">
            <input className="g-input" value={where} onChange={(e) => setWhere(e.target.value)} />
            <button type="button" className="g-btn hub-btn" onClick={() => setPicking(true)}>
              <FolderSearch size={14} /> Browse…
            </button>
          </div>
        </label>
        <div className="hub-dim">
          Creates <code>{`${where.replace(/[\\/]+$/, '')}/${name.trim()}`}</code>
        </div>
        {error && <div className="hub-error">{error}</div>}
        <div className="hub-dialog-buttons">
          <button type="button" className="g-btn hub-btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="submit" className="g-btn primary hub-btn" disabled={invalid || !where || busy}>
            {busy ? 'Creating…' : 'Create project'}
          </button>
        </div>
        {picking && (
          <FolderPicker
            title="Choose a location"
            initial={where}
            mode="folder"
            onClose={() => setPicking(false)}
            onPick={(p) => {
              setWhere(p);
              setPicking(false);
            }}
          />
        )}
      </form>
    </div>
  );
}

function FolderPicker({ title, initial, mode, onPick, onClose }: { title: string; initial: string; mode: 'project' | 'folder'; onPick: (path: string) => void; onClose: () => void }) {
  const [listing, setListing] = useState<DirListing | null>(null);
  const [pathText, setPathText] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const go = useCallback((p: string) => {
    setError(null);
    invoke<DirListing>('hub_ls', { path: p }).then(
      (l) => {
        setListing(l);
        setPathText(l.path);
      },
      (e) => {
        setError(err(e));
        if (p) void invoke<DirListing>('hub_ls', { path: '' }).then(setListing);
      },
    );
  }, []);
  useEffect(() => go(initial), [go, initial]);
  const canPick = !!listing?.path && (mode === 'folder' || listing.isProject);
  return (
    <div className="hub-overlay" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="hub-dialog">
        <div className="hub-dialog-title">
          {title}
          <div className="hub-flex" />
          <button type="button" className="hub-icon-btn" onClick={onClose}>
            <X size={15} />
          </button>
        </div>
        <div className="hub-field-row">
          <button type="button" className="hub-icon-btn" title="Up" disabled={!listing || listing.parent === null} onClick={() => listing && go(listing.parent ?? '')}>
            <ArrowUp size={15} />
          </button>
          <input
            className="g-input"
            value={pathText}
            placeholder="Folder path"
            onChange={(e) => setPathText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                go(pathText);
              }
            }}
          />
        </div>
        <div className="hub-roots">
          {listing?.roots.map((r) => (
            <button type="button" key={r.path} className="hub-root" onClick={() => go(r.path)}>
              {r.label === 'Home' ? <Home size={12} /> : <HardDrive size={12} />} {r.label}
            </button>
          ))}
        </div>
        <div className="hub-folders">
          {!listing && !error && <div className="hub-empty">Loading…</div>}
          {listing?.entries.length === 0 && <div className="hub-empty">No sub-folders.</div>}
          {listing?.entries.map((e) => (
            <div
              key={e.path}
              className={cls('hub-folder', e.isProject && 'project')}
              onClick={() => go(e.path)}
              onDoubleClick={() => mode === 'project' && e.isProject && onPick(e.path)}
              title={e.isProject ? 'GoatEngine project' : e.path}
            >
              {e.isProject ? <Box size={14} /> : <Folder size={14} />}
              <span>{e.name}</span>
              {e.isProject && <span className="hub-tag">Project</span>}
            </div>
          ))}
        </div>
        {error && <div className="hub-error">{error}</div>}
        <div className="hub-dialog-buttons">
          <span className="hub-dim hub-flex">{mode === 'project' ? (listing?.isProject ? 'This folder is a GoatEngine project.' : 'Choose a folder containing goat.project.json.') : listing?.path}</span>
          <button type="button" className="g-btn hub-btn" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="g-btn primary hub-btn" disabled={!canPick} onClick={() => listing && onPick(listing.path)}>
            {mode === 'project' ? 'Open project' : 'Select folder'}
          </button>
        </div>
      </div>
    </div>
  );
}
