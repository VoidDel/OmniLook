// Shared Omni design system (tokens, base, window shell, components), then this app's own styles.
import 'omni-ui/styles.css';
import './styles/app.css';

import { getCurrentWindow } from '@tauri-apps/api/window';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { initI18n, resolveLang } from './i18n';
import { api } from './lib/api';
import { detectPlatform } from './lib/platform';
import { DEFAULT_APPEARANCE, applyTheme } from './lib/theme';
import { SettingsApp } from './settings/SettingsApp';
import { ViewApp } from './view/ViewApp';

const platform = detectPlatform(navigator.userAgent);

// The WebView's own context menu is not Omni Look's (text fields keep theirs).
document.addEventListener('contextmenu', (e) => {
  if (!(e.target as HTMLElement).closest('input, textarea')) e.preventDefault();
});

// The same page serves the preview window (`view`) and the settings window (`settings`), each in
// its own process. Both start hidden and show once their first frame is drawn.
const label = getCurrentWindow().label;
const settings = await api.getSettings().catch(() => null);
applyTheme(settings?.appearance ?? DEFAULT_APPEARANCE, platform);
await initI18n(resolveLang(settings?.language ?? 'auto'));

createRoot(document.getElementById('root')!).render(
  <StrictMode>{label === 'view' ? <ViewApp /> : <SettingsApp platform={platform} />}</StrictMode>,
);
