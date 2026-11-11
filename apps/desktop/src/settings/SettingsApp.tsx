// The settings window (docs/development-plan.md §1.2): the hotkey, starting with Windows, the
// language and the appearance. Every change is saved at once; the agent reloads the hotkey.

import { getCurrentWindow } from '@tauri-apps/api/window';
import { Icon, Segmented, Select, Switch, builtinThemes, type Platform } from 'omni-ui';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { LANGUAGE_NAMES, SUPPORTED, resolveLang, setLanguage } from '../i18n';
import { api, type Appearance, type Settings } from '../lib/api';
import { nextFrame } from '../lib/frame';
import { DEFAULT_APPEARANCE, applyTheme } from '../lib/theme';

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
      <main className="settings-body">
        <section className="set-group">
          <h2>{t('settings.general')}</h2>
          <div className="set-row">
            <div>
              <div className="lbl">{t('settings.hotkey')}</div>
              <div className="hint">{t('settings.hotkeyHint')}</div>
            </div>
            <Segmented
              value={s.hotkey}
              label={t('settings.hotkey')}
              options={[
                ['ctrlSpace', 'Ctrl+Space'],
                ['space', t('settings.hotkeySpace')],
              ]}
              onChange={(hotkey) => save({ ...s, hotkey })}
            />
          </div>
          <div className="set-row">
            <div>
              <div className="lbl">{t('settings.autostart')}</div>
              <div className="hint">{t('settings.autostartHint')}</div>
            </div>
            <Switch checked={s.autostart} label={t('settings.autostart')} onChange={(autostart) => save({ ...s, autostart })} />
          </div>
        </section>
        <section className="set-group">
          <h2>{t('settings.appearance')}</h2>
          <div className="set-row">
            <div className="lbl">{t('settings.language')}</div>
            <Select
              value={s.language}
              label={t('settings.language')}
              options={[['auto', t('settings.languageAuto')], ...SUPPORTED.map((l): [string, string] => [l, LANGUAGE_NAMES[l]])]}
              onChange={(language) => {
                save({ ...s, language });
                void setLanguage(resolveLang(language));
              }}
            />
          </div>
          <div className="set-row">
            <div className="lbl">{t('settings.mode')}</div>
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
          </div>
          <div className="set-row">
            <div className="lbl">{t('settings.lightTheme')}</div>
            <Select value={look.lightTheme} label={t('settings.lightTheme')} options={themeOptions(false)} onChange={(lightTheme) => setLook({ lightTheme })} />
          </div>
          <div className="set-row">
            <div className="lbl">{t('settings.darkTheme')}</div>
            <Select value={look.darkTheme} label={t('settings.darkTheme')} options={themeOptions(true)} onChange={(darkTheme) => setLook({ darkTheme })} />
          </div>
        </section>
        <p className="settings-foot">Omni Look {version || __APP_VERSION__}</p>
      </main>
    </div>
  );
}
