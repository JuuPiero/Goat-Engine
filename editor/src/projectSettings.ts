// assets/project.json (Unity's ProjectSettings + Build Settings' scene list), shared by the
// Build Settings / Project Settings windows and the Inspector's Tag / Layer lists.
import { useSyncExternalStore } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api } from './api';
import { getEngine } from './engine';
import { log } from './store';

export interface SceneEntry {
  path: string;
  enabled: boolean;
}
export interface ProjectSettings {
  name: string;
  version: string;
  scenes: SceneEntry[];
  fixed_timestep: number;
  max_delta: number;
  gravity: [number, number, number];
  gravity_2d: [number, number];
  tags: string[];
  layers: string[];
  [extra: string]: unknown;
}

export const KEY = 'project.json';

export function defaults(): ProjectSettings {
  return { name: 'Game', version: '1.0', scenes: [], fixed_timestep: 1 / 60, max_delta: 0.1, gravity: [0, -9.81, 0], gravity_2d: [0, -9.81], tags: [], layers: ['Default'] };
}

let current: ProjectSettings = defaults();
let loaded = false;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

export async function loadSettings(): Promise<ProjectSettings> {
  try {
    const text = await api.readText(KEY);
    current = { ...defaults(), ...JSON.parse(text.replace(/^﻿/, '')) };
  } catch {
    current = defaults();
  }
  loaded = true;
  emit();
  return current;
}

/// Writes project.json (and hands it to the in-editor engine so Play mode sees it at once).
export async function saveSettings(next: ProjectSettings) {
  current = next;
  emit();
  const text = JSON.stringify(next, null, 2) + '\n';
  try {
    await api.writeText(KEY, text);
    getEngine()?.put_file(KEY, new TextEncoder().encode(text));
  } catch (e) {
    log(`project.json: ${e}`, 'error');
  }
}

export function useSettings(): ProjectSettings {
  if (!loaded) {
    loaded = true;
    void loadSettings();
    // edited outside the editor (or by another editor window)
    void listen('assets-changed', () => void loadSettings());
  }
  return useSyncExternalStore(
    (l) => (listeners.add(l), () => listeners.delete(l)),
    () => current,
  );
}

export function settings() {
  return current;
}
