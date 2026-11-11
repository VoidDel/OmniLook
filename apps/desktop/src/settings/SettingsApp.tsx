// The settings window (docs/development-plan.md §1.2): the hotkey, starting with Windows, the
// notification-area icon, the language and the appearance. Every change is saved at once; the
// agent reloads what it uses (hotkey, icon).

import { getCurrentWindow } from '@tauri-apps/api/window';
import { Icon, Segmented, Select, Switch, builtinThemes, type Platform } from 'omni-ui';
import { type ReactNode, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { LANGUAGE_NAMES, SUPPORTED, resolveLang, setLanguage } from '../i18n';
import { api, type Appearance, type Settings } from '../lib/api';
import { nextFrame } from '../lib/frame';
import { DEFAULT_APPEARANCE, applyTheme } from '../lib/theme';

/** One setting: its label and hint on the left, the control on the right (omni-ui's layout). */
function Row({ label, hint, children }: { label: ReactNode; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="set-row">
      <div className="lab">
        <span>{label}</span>
        {hint && <small>{hint}</small>}
      </div>
      <div className="ctl">{children}</div>
    </div>
  );
}

export function SettingsApp({ platform }: { platform: Platform }) {
  const { t, i18n } = useTranslation();
  const [s, setS] = useState<Settings | null>(null);
  const [version, setVersion] = useState('');

  useEffect(() => {
    void api.getSettings().then(setS);
    void api.appVersion().then(setVersion);
    nextFrame(() =>
      nextFrame(() => {
        const w = getCurrentWindow();
        void w.show().then(() => w.setFocus());
      }),
    );
  }, []);

  if (!s) return null;
  const look = s.appearance ?? DEFAULT_APPEARANCE;
  const save = (next: Settings) => {
    setS(next);
    void api.saveSettings(next);
  };
  const setLook = (patch: Partial<Appearance>) => {
    const a = { ...look, ...patch };
    applyTheme(a, platform, true);
    save({ ...s, appearance: a });
  };
  const lang = i18n.language;
  const themeOptions = (dark: boolean): [string, string][] =>
    builtinThemes
      .filter((th) => (th.appearance === 'dark') === dark)
      .map((th) => [th.id, th.name[lang] ?? th.name['en-US'] ?? th.id]);

  return (
    <div className="settings">
      <header className="look-bar" data-tauri-drag-region>
        <div className="look-name" data-tauri-drag-region>
          <Icon name="eye" />
          <b data-tauri-drag-region>{t('settings.title')}</b>
        </div>
        <div className="winctl">
          <button type="button" onClick={() => void getCurrentWindow().minimize()} aria-label={t('common.minimize')}>
            <Icon name="minus" />
          </button>
          <button type="button" className="close" onClick={() => void getCurrentWindow().close()} aria-label={t('common.close')}>
            <Icon name="x" />
          </button>
        </div>
      </header>
      <main className="set-body">
        <h2>{t('settings.general')}</h2>
        <div className="set-group">
          <Row label={t('settings.hotkey')} hint={t('settings.hotkeyHint')}>
            <Segmented
              value={s.hotkey}
              label={t('settings.hotkey')}
              options={[
                ['ctrlSpace', 'Ctrl+Space'],
                ['space', t('settings.hotkeySpace')],
              ]}
              onChange={(hotkey) => save({ ...s, hotkey })}
            />
          </Row>
          <Row label={t('settings.closeOnBlur')} hint={t('settings.closeOnBlurHint')}>
            <Switch checked={s.closeOnBlur} label={t('settings.closeOnBlur')} onChange={(closeOnBlur) => save({ ...s, closeOnBlur })} />
          </Row>
          <Row label={t('settings.autostart')} hint={t('settings.autostartHint')}>
            <Switch checked={s.autostart} label={t('settings.autostart')} onChange={(autostart) => save({ ...s, autostart })} />
          </Row>
          <Row label={t('settings.tray')} hint={t('settings.trayHint')}>
            <Switch checked={s.tray} label={t('settings.tray')} onChange={(tray) => save({ ...s, tray })} />
          </Row>
        </div>
        <h2>{t('settings.appearance')}</h2>
        <div className="set-group">
          <Row label={t('settings.language')}>
            <Select
              value={s.language}
              label={t('settings.language')}
              options={[['auto', t('settings.languageAuto')], ...SUPPORTED.map((l): [string, string] => [l, LANGUAGE_NAMES[l]])]}
              onChange={(language) => {
                save({ ...s, language });
                void setLanguage(resolveLang(language));
              }}
            />
          </Row>
          <Row label={t('settings.mode')}>
            <Segmented
              value={look.mode}
              label={t('settings.mode')}
              options={[
                ['system', t('settings.modeSystem')],
                ['light', t('settings.modeLight')],
                ['dark', t('settings.modeDark')],
              ]}
              onChange={(mode) => setLook({ mode })}
            />
          </Row>
          <Row label={t('settings.lightTheme')}>
            <Select value={look.lightTheme} label={t('settings.lightTheme')} options={themeOptions(false)} onChange={(lightTheme) => setLook({ lightTheme })} />
          </Row>
          <Row label={t('settings.darkTheme')}>
            <Select value={look.darkTheme} label={t('settings.darkTheme')} options={themeOptions(true)} onChange={(darkTheme) => setLook({ darkTheme })} />
          </Row>
        </div>
        <p className="settings-foot">Omni Look {version || __APP_VERSION__}</p>
      </main>
    </div>
  );
}
