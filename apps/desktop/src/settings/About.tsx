// Settings › About: the version and updates (Windows: checked when the settings open, installed
// when asked; elsewhere a link to the releases), where to report a problem, the project page.

import { listen } from '@tauri-apps/api/event';
import { Switch, type Platform } from 'omni-ui';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type Settings, type UpdateInfo } from '../lib/api';

const REPO = 'https://github.com/VoidDel/OmniLook';

type Check =
  | { state: 'idle' }
  | { state: 'checking' }
  | { state: 'latest' }
  | { state: 'available'; update: UpdateInfo }
  | { state: 'installing'; percent: number | null }
  | { state: 'failed'; error: string };

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

export function About({ s, save, platform, version }: { s: Settings; save: (s: Settings) => void; platform: Platform; version: string }) {
  const { t } = useTranslation();
  const [check, setCheck] = useState<Check>({ state: 'idle' });
  const auto = useRef(false);
  const win = platform === 'win';

  const run = async (quiet: boolean) => {
    setCheck({ state: 'checking' });
    try {
      const u = await api.updateCheck();
      setCheck(u ? { state: 'available', update: u } : quiet ? { state: 'idle' } : { state: 'latest' });
    } catch (e) {
      setCheck(quiet ? { state: 'idle' } : { state: 'failed', error: String(e) });
    }
  };

  // Once when the settings open (Windows, when allowed): quiet unless there is something new.
  useEffect(() => {
    if (!win || !s.checkUpdates || auto.current) return;
    auto.current = true;
    void run(true);
  }, [win, s.checkUpdates]);

  const install = async () => {
    setCheck({ state: 'installing', percent: null });
    const off = await listen<{ downloaded: number; total: number | null }>('update:progress', (e) => {
      const { downloaded, total } = e.payload;
      setCheck({ state: 'installing', percent: total ? Math.round((downloaded / total) * 100) : null });
    });
    try {
      await api.updateInstall();
    } catch (e) {
      setCheck({ state: 'failed', error: String(e) });
    } finally {
      off();
    }
  };

  const status = (() => {
    switch (check.state) {
      case 'checking':
        return t('about.checking');
      case 'latest':
        return t('about.latest');
      case 'available':
        return t('about.available', { version: check.update.version });
      case 'installing':
        return check.percent == null ? t('about.downloading') : t('about.downloadingPercent', { percent: check.percent });
      case 'failed':
        return t('about.failed', { error: check.error });
      default:
        return null;
    }
  })();

  return (
    <>
      <h2>{t('about.title')}</h2>
      <div className="set-group">
        <Row label={`Omni Look ${version}`} hint={status}>
          {!win ? (
            <button type="button" className="btn" onClick={() => void api.openUrl(`${REPO}/releases`)}>
              {t('about.releases')}
            </button>
          ) : check.state === 'available' ? (
            <button type="button" className="btn primary" onClick={() => void install()}>
              {t('about.install', { version: check.update.version })}
            </button>
          ) : (
            <button type="button" className="btn" disabled={check.state === 'checking' || check.state === 'installing'} onClick={() => void run(false)}>
              {t('about.check')}
            </button>
          )}
        </Row>
        {win && (
          <Row label={t('about.auto')} hint={t('about.autoHint')}>
            <Switch checked={s.checkUpdates} label={t('about.auto')} onChange={(checkUpdates) => save({ ...s, checkUpdates })} />
          </Row>
        )}
        <Row label={t('about.feedback')} hint={t('about.feedbackHint')}>
          <button type="button" className="btn" onClick={() => void api.openUrl(`${REPO}/issues/new/choose`)}>
            {t('about.report')}
          </button>
        </Row>
        <Row label={t('about.home')} hint={REPO.replace('https://', '')}>
          <button type="button" className="btn" onClick={() => void api.openUrl(REPO)}>
            {t('about.open')}
          </button>
        </Row>
      </div>
    </>
  );
}
