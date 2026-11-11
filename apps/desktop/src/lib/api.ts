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
  /** The notification-area icon. */
  tray: boolean;
  /** Close the preview when another window comes to the front. */
  closeOnBlur: boolean;
  language: string;
  /** Null until saved once. */
  appearance: Appearance | null;
}

export interface TextPreview {
  text: string;
  truncated: boolean;
  encoding: string;
  language: string | null;
}

export interface HexPreview {
  dump: string;
  truncated: boolean;
}

export interface FolderItem {
  name: string;
  dir: boolean;
  size: number;
  modified: number | null;
}

export interface FolderPreview {
  items: FolderItem[];
  files: number;
  folders: number;
  bytes: number;
  complete: boolean;
}

export interface ArchiveEntry {
  path: string;
  dir: boolean;
  size: number;
  encrypted: boolean;
}

export interface ArchivePreview {
  format: string;
  entries: ArchiveEntry[];
  files: number;
  folders: number;
  size: number;
  encrypted: boolean;
  truncated: boolean;
  error: string | null;
}

export const api = {
  viewReady: () => invoke<Shown | null>('view_ready'),
  viewShow: () => invoke<void>('view_show'),
  viewClose: () => invoke<void>('view_close'),
  viewNav: (step: number) => invoke<void>('view_nav', { step }),
  openDefault: () => invoke<boolean>('open_default'),
  readText: (id: number) => invoke<TextPreview>('read_text', { id }),
  /** Flat [start, length, class] triples in UTF-16 units. */
  highlight: (id: number) => invoke<number[]>('highlight', { id }),
  readHex: (id: number) => invoke<HexPreview>('read_hex', { id }),
  readFolder: (id: number) => invoke<FolderPreview>('read_folder', { id }),
  readArchive: (id: number, locale: string) => invoke<ArchivePreview>('read_archive', { id, locale }),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  appVersion: () => invoke<string>('app_version'),
};
