// Asset type icons (inline SVG, no image files).
const COLORS: Record<string, string> = {
  folder: '#d9b45a',
  scene: '#6fb2ff',
  prefab: '#5ad1e6',
  material: '#e07ad1',
  controller: '#ffa94d',
  image: '#8bd36b',
  model: '#f09a4a',
  audio: '#c9a2ff',
  font: '#e6e6e6',
  text: '#b8bec9',
  file: '#9aa1ad',
};

export function AssetIcon({ kind, size }: { kind: string; size: number }) {
  const c = COLORS[kind] ?? COLORS.file;
  const s = { width: size, height: size };
  switch (kind) {
    case 'folder':
      return (
        <svg viewBox="0 0 24 24" style={s}>
          <path d="M2 6a2 2 0 0 1 2-2h5l2 2h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2z" fill={c} />
        </svg>
      );
    case 'scene':
      return (
        <svg viewBox="0 0 24 24" style={s}>
          <rect x="3" y="3" width="18" height="18" rx="3" fill="none" stroke={c} strokeWidth="2" />
          <path d="M6 17l4-5 3 3 2-2 3 4z" fill={c} />
          <circle cx="16" cy="8" r="2" fill={c} />
        </svg>
      );
    case 'prefab':
    case 'model':
      return (
        <svg viewBox="0 0 24 24" style={s}>
          <path d="M12 2l9 5v10l-9 5-9-5V7z" fill="none" stroke={c} strokeWidth="2" strokeLinejoin="round" />
          <path d="M3 7l9 5 9-5M12 12v10" fill="none" stroke={c} strokeWidth="2" />
        </svg>
      );
    case 'material':
      return (
        <svg viewBox="0 0 24 24" style={s}>
          <defs>
            <radialGradient id="mg" cx="35%" cy="35%">
              <stop offset="0" stopColor="#fff" />
              <stop offset="1" stopColor={c} />
            </radialGradient>
          </defs>
          <circle cx="12" cy="12" r="9" fill="url(#mg)" />
        </svg>
      );
    case 'controller':
      return (
        <svg viewBox="0 0 24 24" style={s}>
          <rect x="2" y="4" width="8" height="5" rx="1.5" fill={c} />
          <rect x="14" y="4" width="8" height="5" rx="1.5" fill="none" stroke={c} strokeWidth="1.8" />
          <rect x="8" y="15" width="8" height="5" rx="1.5" fill="none" stroke={c} strokeWidth="1.8" />
          <path d="M10 6.5h4M6 9l4 6M18 9l-4 6" stroke={c} strokeWidth="1.8" fill="none" />
        </svg>
      );
    case 'audio':
      return (
        <svg viewBox="0 0 24 24" style={s}>
          <path d="M4 9h4l5-4v14l-5-4H4z" fill={c} />
          <path d="M16 8a5 5 0 0 1 0 8M18.5 5.5a8.5 8.5 0 0 1 0 13" fill="none" stroke={c} strokeWidth="2" />
        </svg>
      );
    case 'font':
      return (
        <svg viewBox="0 0 24 24" style={s}>
          <text x="12" y="18" textAnchor="middle" fontSize="16" fontWeight="bold" fill={c} fontFamily="serif">
            Aa
          </text>
        </svg>
      );
    default:
      return (
        <svg viewBox="0 0 24 24" style={s}>
          <path d="M6 2h8l5 5v15H6z" fill="none" stroke={c} strokeWidth="2" strokeLinejoin="round" />
          <path d="M14 2v5h5M9 12h6M9 16h6" stroke={c} strokeWidth="2" />
        </svg>
      );
  }
}
