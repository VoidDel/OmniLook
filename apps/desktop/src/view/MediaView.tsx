// Video and sound (docs/development-plan.md §5.1): played from look:// (ranges, so seeking works).
// Sound gets its cover, title, artist and album beside the player; a format the window cannot
// play (AVI, WMV, WMA…) gets its details and "open" instead of a player that fails.

import { FileIcon } from 'omni-ui';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type AudioInfo, type Shown } from '../lib/api';
import { formatDuration, formatSize } from '../lib/format';

/** Length, bitrate, sample rate and channels, for the status line. */
function useAudioMeta(shown: Shown, onMeta: (meta: string) => void) {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AudioInfo | null>(null);
  useEffect(() => {
    let gone = false;
    void api.readAudio(shown.id).then(
      (r) => !gone && setInfo(r),
      () => !gone && setInfo(null),
    );
    return () => {
      gone = true;
    };
  }, [shown.id]);
  useEffect(() => {
    if (!info) return;
    const parts: string[] = [];
    if (info.durationMs) parts.push(formatDuration(info.durationMs));
    if (info.bitrate) parts.push(`${info.bitrate} kbps`);
    if (info.sampleRate) parts.push(`${(info.sampleRate / 1000).toFixed(info.sampleRate % 1000 ? 1 : 0)} kHz`);
    if (info.channels) parts.push(info.channels === 1 ? t('view.mono') : info.channels === 2 ? t('view.stereo') : t('view.channels', { count: info.channels }));
    onMeta(parts.join(' · '));
  }, [info, onMeta, t]);
  return info;
}

/** Cannot be played here: the file and what is known about it, and "open". */
function Unplayable({ shown, info, failed }: { shown: Shown; info: AudioInfo | null; failed?: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="look-fallback">
      <FileIcon name={shown.name} isDir={false} big />
      <b>{info?.title ?? shown.name}</b>
      <span>
        {[info?.artist, info?.durationMs ? formatDuration(info.durationMs) : null, formatSize(shown.size)].filter(Boolean).join(' · ')}
      </span>
      <span className="look-fallback-note">{failed ? t('view.playFailed') : t('view.cannotPlay')}</span>
      <button type="button" className="btn primary" onClick={() => void api.openDefault()}>
        {t('view.openDefault')}
      </button>
    </div>
  );
}

export function MediaView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const video = shown.kind === 'video';
  const info = useAudioMeta(shown, onMeta);
  const [failed, setFailed] = useState(false);
  const [coverFailed, setCoverFailed] = useState(false);
  if (!shown.playable || failed) return <Unplayable shown={shown} info={info} failed={failed} />;
  if (video) {
    return (
      <div className="look-media">
        <video src={shown.url} controls autoPlay onError={() => setFailed(true)} />
      </div>
    );
  }
  const cover = info?.hasCover && !coverFailed ? shown.url.replace('/f/', '/c/') : null;
  return (
    <div className="look-audio">
      <div className="look-cover">
        {cover ? <img src={cover} alt="" draggable={false} onError={() => setCoverFailed(true)} /> : <FileIcon name={shown.name} isDir={false} big />}
      </div>
      <div className="look-tags">
        <b>{info?.title ?? shown.name}</b>
        {info?.artist && <span>{info.artist}</span>}
        {info?.album && <span className="dim">{info.album}</span>}
      </div>
      <audio src={shown.url} controls autoPlay onError={() => setFailed(true)} />
    </div>
  );
}
