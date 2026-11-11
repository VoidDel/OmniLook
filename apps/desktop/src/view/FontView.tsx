// Fonts (docs/development-plan.md §5.1, M3): the font loaded from look:// with the FontFace API
// and shown in samples: its name, the alphabet and digits, a sentence at several sizes, and the
// scripts it covers (from Rust). The sample sentence can be typed over.

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type FontInfo, type Shown } from '../lib/api';

const SIZES = [12, 16, 24, 36, 48, 72];

export function FontView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t, i18n } = useTranslation();
  const [info, setInfo] = useState<FontInfo | null>(null);
  const [face, setFace] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [sample, setSample] = useState('');

  useEffect(() => {
    let gone = false;
    void api.readFont(shown.id, i18n.language).then((r) => !gone && setInfo(r));
    // A family name of its own, so it never clashes with an installed font of the same name.
    const name = `omnilook-preview-${shown.id}`;
    const f = new FontFace(name, `url("${shown.url}")`);
    f.load().then(
      (loaded) => {
        if (gone) return;
        document.fonts.add(loaded);
        setFace(name);
      },
      () => !gone && setFailed(true),
    );
    return () => {
      gone = true;
      for (const x of document.fonts) if (x.family === name) document.fonts.delete(x);
    };
  }, [shown.id, shown.url, i18n.language]);

  useEffect(() => {
    if (!info) return;
    const parts = [info.style, info.version, info.glyphs ? t('font.glyphs', { count: info.glyphs }) : null, info.monospace ? t('font.monospace') : null];
    onMeta(parts.filter(Boolean).join(' · '));
  }, [info, onMeta, t]);

  if (failed) return <div className="look-note">{t('font.failed')}</div>;
  if (!face) return <div className="look-note">{t('view.loading')}</div>;
  const style = { fontFamily: `"${face}"` };
  const scripts = [
    info?.latin !== false && t('font.sampleLatin'),
    info?.cjk && t('font.sampleCjk'),
    info?.kana && t('font.sampleKana'),
    info?.hangul && t('font.sampleHangul'),
  ].filter(Boolean) as string[];
  const sentence = sample || scripts.join(' ');
  return (
    <div className="look-md-scroll">
      <div className="look-font">
        <header>
          <h1 style={style}>{info?.family ?? shown.name.replace(/\.[^.]+$/, '')}</h1>
          <span className="meta">
            {[info?.familyEn, info?.style, info?.designer].filter(Boolean).join(' · ')}
          </span>
        </header>
        <input
          className="inp"
          value={sample}
          placeholder={t('font.typeHere')}
          aria-label={t('font.typeHere')}
          spellCheck={false}
          onChange={(e) => setSample(e.target.value)}
        />
        <div className="glyphs" style={style}>
          {info?.latin !== false && (
            <>
              <p>ABCDEFGHIJKLMNOPQRSTUVWXYZ</p>
              <p>abcdefghijklmnopqrstuvwxyz</p>
              <p>0123456789 !?&amp;@#%()[]{'{}'}.,;:'"+-*/=</p>
            </>
          )}
          {info?.cjk && <p>永字八法 天地玄黄 宇宙洪荒 中文字体预览</p>}
        </div>
        <div className="sizes">
          {SIZES.map((s) => (
            <div key={s} className="size-row">
              <span className="pt">{s}</span>
              <p style={{ ...style, fontSize: `${s}px` }}>{sentence}</p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
