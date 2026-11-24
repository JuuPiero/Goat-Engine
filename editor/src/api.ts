// Typed calls into the Rust side (editor/src-tauri/src/main.rs).
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

export interface Entry {
  name: string;
  key: string;
  is_dir: boolean;
  size: number;
  modified: number;
  kind: string;
}
export interface Folder {
  name: string;
  key: string;
  children: Folder[];
}
export interface ProjectInfo {
  name: string;
  root: string;
  assets: string;
}

export const api = {
  projectInfo: () => invoke<ProjectInfo>('project_info'),
  list: (dir: string) => invoke<Entry[]>('list_dir', { dir }),
  tree: () => invoke<Folder>('folder_tree'),
  search: (query: string) => invoke<Entry[]>('search', { query }),
  absPaths: (keys: string[]) => invoke<string[]>('abs_paths', { keys }),
  keysOf: (paths: string[]) => invoke<(string | null)[]>('keys_of', { paths }),
  move: (keys: string[], dest: string) => invoke<string[]>('move_items', { keys, dest }),
  copyPaths: (paths: string[], dest: string) => invoke<string[]>('copy_paths', { paths, dest }),
  rename: (key: string, name: string) => invoke<string>('rename_item', { key, name }),
  duplicate: (keys: string[]) => invoke<string[]>('duplicate_items', { keys }),
  remove: (keys: string[]) => invoke<void>('delete_items', { keys }),
  createFolder: (dir: string, name: string) => invoke<string>('create_folder', { dir, name }),
  createAsset: (dir: string, kind: string) => invoke<string>('create_asset', { dir, kind }),
  readText: (key: string) => invoke<string>('read_text', { key }),
  writeText: (key: string, text: string) => invoke<void>('write_text', { key, text }),
  reveal: (key: string) => invoke<void>('reveal', { key }),
  buildGame: (platform: string, startScene: string | null, debug: boolean) => invoke<void>('build_game', { platform, startScene, debug }),
  dragIcon: () => invoke<string>('drag_icon'),
};

/// Absolute path of a key (the assets root uses the OS separator).
export function absPath(assetsRoot: string, key: string): string {
  const sep = assetsRoot.includes('\\') ? '\\' : '/';
  return key ? assetsRoot + sep + key.split('/').join(sep) : assetsRoot;
}

/// URL a webview can load for a project file (images: thumbnails, previews).
export function fileUrl(assetsRoot: string, key: string, version = 0): string {
  return convertFileSrc(absPath(assetsRoot, key)) + (version ? `?v=${version}` : '');
}

export const parentOf = (key: string) => (key.includes('/') ? key.slice(0, key.lastIndexOf('/')) : '');
export const nameOf = (key: string) => key.slice(key.lastIndexOf('/') + 1);
