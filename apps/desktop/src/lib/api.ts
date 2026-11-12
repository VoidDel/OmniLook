// The Rust side (apps/desktop/src-tauri/src/ui/mod.rs): one function per command.

import { invoke } from '@tauri-apps/api/core';

import type { RecordSummary } from './records';

export type Kind = 'text' | 'markdown' | 'image' | 'svg' | 'pdf' | 'video' | 'audio' | 'archive' | 'folder' | 'handler' | 'office' | 'font' | 'cad' | 'binary';

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
  /** Media the window can play (AVI, WMV, WMA… cannot). */
  playable: boolean;
}

/** A sound file's tags and format (crates/omnilook-core/src/audio.rs). */
export interface AudioInfo {
  title: string | null;
  artist: string | null;
  album: string | null;
  durationMs: number | null;
  /** kbit/s. */
  bitrate: number | null;
  sampleRate: number | null;
  channels: number | null;
  /** The cover is at the file's URL with /c/ for /f/. */
  hasCover: boolean;
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
  /** Look for a new version once a day and when the settings open (Windows). */
  checkUpdates: boolean;
  language: string;
  /** Null until saved once. */
  appearance: Appearance | null;
}

/** A newer release (src-tauri/src/update.rs). */
export interface UpdateInfo {
  version: string;
  notes: string;
}

export interface TextPreview {
  text: string;
  truncated: boolean;
  encoding: string;
  language: string | null;
  /** Firmware records (Intel HEX, S-records) found in the whole file. */
  records: RecordSummary | null;
}

/** Where a system preview handler draws, in window pixels. */
export interface HostArea {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A font's names and the scripts it covers (crates/omnilook-core/src/font.rs). */
export interface FontInfo {
  family: string | null;
  familyEn: string | null;
  style: string | null;
  version: string | null;
  designer: string | null;
  glyphs: number | null;
  monospace: boolean;
  latin: boolean;
  cjk: boolean;
  kana: boolean;
  hangul: boolean;
}

/** Word / Excel / PowerPoint text without Office (crates/omnilook-core/src/office.rs). */
export interface OfficeRun {
  text: string;
  bold?: boolean;
  italic?: boolean;
}
export type OfficeBlock = { kind: 'para'; level: number; list: boolean; runs: OfficeRun[] } | { kind: 'table'; rows: string[][] };
export interface OfficeSheet {
  name: string;
  rows: string[][];
  truncated: boolean;
}
export type OfficePreview =
  | { type: 'word'; blocks: OfficeBlock[]; truncated: boolean }
  | { type: 'excel'; sheets: OfficeSheet[] }
  | { type: 'powerPoint'; slides: string[][]; truncated: boolean };

/** Markdown rendered in Rust and cleaned there (crates/omnilook-core/src/markdown.rs). */
export interface MarkdownPreview {
  html: string;
  truncated: boolean;
  encoding: string;
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
  readFont: (id: number, locale: string) => invoke<FontInfo>('read_font', { id, locale }),
  readOffice: (id: number) => invoke<OfficePreview>('read_office', { id }),
  /** A CAD drawing, as bytes (lib/cad.ts `decodeDrawing`). */
  readCad: (id: number) => invoke<ArrayBuffer>('read_cad', { id }),
  readAudio: (id: number) => invoke<AudioInfo>('read_audio', { id }),
  /** A system preview handler over the page's area (window pixels); colours as 0x00BBGGRR. */
  hostShow: (id: number, area: HostArea, bg: number | null, fg: number | null) => invoke<string>('host_show', { id, ...area, bg, fg }),
  hostArea: (area: HostArea) => invoke<void>('host_area', { ...area }),
  hostUnload: () => invoke<void>('host_unload'),
  readMarkdown: (id: number) => invoke<MarkdownPreview>('read_markdown', { id }),
  /** A link from a preview: web and mail addresses only (refused otherwise). */
  openUrl: (url: string) => invoke<boolean>('open_url', { url }),
  readFolder: (id: number) => invoke<FolderPreview>('read_folder', { id }),
  readArchive: (id: number, locale: string) => invoke<ArchivePreview>('read_archive', { id, locale }),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  appVersion: () => invoke<string>('app_version'),
  updateCheck: () => invoke<UpdateInfo | null>('update_check'),
  /** Downloads and installs; progress arrives as `update:progress` events. */
  updateInstall: () => invoke<void>('update_install'),
};
