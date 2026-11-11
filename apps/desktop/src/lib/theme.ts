// Theme: one of omni-ui's built-in themes for light and one for dark, resolved in the page and
// written to the document, as in OmniZip and Omni Download.

import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  builtinTheme,
  resolveTheme,
  systemPrefersDark,
  writeTheme,
  writeThemeAnimated,
  type Platform,
} from 'omni-ui';

import type { Appearance } from './api';

export const DEFAULT_APPEARANCE: Appearance = {
  mode: 'system',
  lightTheme: DEFAULT_LIGHT_THEME,
  darkTheme: DEFAULT_DARK_THEME,
  accent: 'theme',
};

export function isDark(a: Appearance): boolean {
  return a.mode === 'dark' || (a.mode === 'system' && systemPrefersDark());
}

/** Resolves the theme of `a` and writes it to the document, cross-fading when `animate`. */
export function applyTheme(a: Appearance, platform: Platform, animate = false): void {
  const dark = isDark(a);
  const theme = builtinTheme(dark ? a.darkTheme : a.lightTheme) ?? builtinTheme(dark ? DEFAULT_DARK_THEME : DEFAULT_LIGHT_THEME);
  if (!theme) return;
  const resolved = resolveTheme(theme, { accent: a.accent || 'theme', systemAccent: null, material: false });
  const opts = { platform, density: 'comfortable' as const, iconStyle: 'duotone' as const, material: false, fontSize: 14 };
  if (animate) writeThemeAnimated(resolved, opts);
  else writeTheme(resolved, opts);
  if (isTauri()) {
    void getCurrentWindow()
      .setTheme(a.mode === 'system' ? null : a.mode)
      .catch(() => undefined);
  }
}
