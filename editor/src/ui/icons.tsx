// Editor icons (lucide, like the PiaEngine editor): asset types for the Project window,
// GameObject icons for the Hierarchy, component icons for the Inspector.
import {
  AppWindow,
  AudioLines,
  Box,
  Boxes,
  Camera,
  Clapperboard,
  Database,
  Diamond,
  File,
  FileBraces,
  FileCode,
  FileImage,
  FileMusic,
  FileType,
  Folder,
  Gamepad2,
  Layers,
  LayoutTemplate,
  Lightbulb,
  Link,
  Move,
  Navigation,
  Orbit,
  Palette,
  Image,
  Shapes,
  Sparkles,
  Spline,
  Square,
  Type,
  Video,
  Volume2,
  Waypoints,
  Wind,
  Workflow,
  type LucideIcon,
} from 'lucide-react';

/** Project window asset kinds (fsops.rs kind_of) → icon + color. */
const ASSETS: Record<string, [LucideIcon, string]> = {
  folder: [Folder, '#e8c46a'],
  scene: [Diamond, '#d6d6d6'],
  prefab: [Box, '#7bb4ff'],
  model: [Shapes, '#9ccc65'],
  material: [Palette, '#f2a65a'],
  controller: [Workflow, '#e8a64a'],
  animation: [Spline, '#e8a64a'],
  script: [FileCode, '#4f9be8'],
  scriptableObject: [Database, '#b39ddb'],
  image: [FileImage, '#80cbc4'],
  audio: [FileMusic, '#f06292'],
  font: [FileType, '#ce93d8'],
  input: [Gamepad2, '#aed581'],
  text: [FileBraces, '#e3c27a'],
  file: [File, '#bdbdbd'],
};

export function assetIcon(kind: string): [LucideIcon, string] {
  return ASSETS[kind] ?? ASSETS.file;
}

/** A Project window glyph: folders filled, assets in their color. */
export function AssetGlyph({ kind, size }: { kind: string; size: number }) {
  const [I, color] = assetIcon(kind);
  if (kind === 'folder') return <Folder size={size * 0.72} strokeWidth={1.4} color={color} fill="#e8c46a33" />;
  return <I size={size * 0.62} strokeWidth={1.4} color={color} />;
}

/** Small list icon (tree rows). */
export function AssetSmallIcon({ kind, size = 14 }: { kind: string; size?: number }) {
  const [I, color] = assetIcon(kind);
  return <I size={size} strokeWidth={1.75} color={kind === 'folder' ? color : undefined} className={kind === 'folder' ? 'g-folder-icon' : undefined} />;
}

/** Hierarchy icons by the engine's `icon` (most important component) and row kind. */
export function ObjectIcon({ icon, kind, size = 14 }: { icon?: string; kind?: string; size?: number }) {
  const color = kind === 'prefab' || kind === 'model' ? '#7bb4ff' : icon === 'empty' ? '#9a9a9a' : undefined;
  const I = icon === 'camera' ? Video : icon === 'light' ? Lightbulb : icon === 'particles' ? Sparkles : icon === 'audio' ? Volume2 : Box;
  return <I size={size} strokeWidth={1.75} color={color} />;
}

/** Inspector component header icons, by component name. */
const COMPONENTS: [RegExp, LucideIcon][] = [
  [/^Transform$/, Move],
  [/Camera/, Camera],
  [/Light/, Lightbulb],
  [/MeshRenderer|MeshFilter|Lod|StaticBatching/, Shapes],
  [/Sprite/, Image],
  [/Particle/, Sparkles],
  [/Line|Trail/, Waypoints],
  [/Audio/, AudioLines],
  [/Rigidbody|Collider|Joint|CharacterController|Force|Effector/, Orbit],
  [/Animat/, Clapperboard],
  [/NavMesh/, Navigation],
  [/^Ui|Canvas|Button|Toggle|Slider|Scroll|Dropdown|InputField|Layout/, LayoutTemplate],
  [/Text/, Type],
  [/Script|Behaviour/, FileCode],
  [/RenderSettings|Skybox|Fog/, Layers],
  [/Wind/, Wind],
  [/Prefab/, Boxes],
  [/Link|Parent/, Link],
  [/Window/, AppWindow],
];

export function ComponentIcon({ name, size = 14 }: { name: string; size?: number }) {
  const I = COMPONENTS.find(([re]) => re.test(name))?.[1] ?? Square;
  return <I size={size} strokeWidth={1.75} />;
}
