// The Rust side (apps/desktop/src-tauri/src/ui/mod.rs): one function per command.

import { invoke } from '@tauri-apps/api/core';

export type Kind = 'text' | 'image' | 'svg' | 'pdf' | 'video' | 'audio' | 'archive' | 'folder' | 'handler' | 'binary';

/** The file a preview shows. */
export interface Shown {
  id: number;
  path: string;
  name: string;
  /** A title given by the caller (OmniZip: "archive › path in it"). */
  title: string | null;
  kind: Kind;
  size: number;
  /** Milliseconds since 1970. */
  modified: number | null;
  /** The file through look://, for images, PDF and media. */
  url: string;
}

export type ThemeMode = 'system' | 'light' | 'dark';

export interface Appearance {
  mode: ThemeMode;
  lightTheme: string;
  darkTheme: string;
  /** `theme`, `system` or a #rrggbb colour. */
  accent: string;
}

export interface Settings {
  hotkey: 'ctrlSpace' | 'space';
  autostart: boolean;
  language: string;
  /** Null until saved once. */
  appearance: Appearance | null;
}

export const api = {
  viewReady: () => invoke<Shown | null>('view_ready'),
  viewShow: () => invoke<void>('view_show'),
  viewClose: () => invoke<void>('view_close'),
  viewNav: (step: number) => invoke<void>('view_nav', { step }),
  openDefault: () => invoke<boolean>('open_default'),
  readText: (id: number) => invoke<string>('read_text', { id }),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  appVersion: () => invoke<string>('app_version'),
};
