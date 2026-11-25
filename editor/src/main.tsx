import '@fontsource-variable/inter';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { App } from './App';
import { HubApp } from './hub/HubApp';
import './styles.css';
import './skin.css';
import './goat.css';

// No project open (started without --project, or File ▸ Open Project…): the Project Hub.
const root = createRoot(document.getElementById('root')!);
invoke<{ project: string | null }>('hub_state').then(
  (s) => root.render(s.project ? <App /> : <HubApp />),
  () => root.render(<App />),
);
