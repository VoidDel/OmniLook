// Markdown, rendered: the HTML comes from Rust already cleaned (crates/omnilook-core/src/markdown.rs:
// no script, style or frames; local images through look://, internet images as links). Links
// open in the browser; in-page links (#section) scroll here.

import { type MouseEvent, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type MarkdownPreview, type Shown } from '../lib/api';

/** The rendered page's ids carry this prefix (`markdown::ID_PREFIX`). */
const ID_PREFIX = 'md-';

export function MarkdownView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t } = useTranslation();
  const [md, setMd] = useState<MarkdownPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const page = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let gone = false;
    api.readMarkdown(shown.id).then(
      (r) => !gone && setMd(r),
      (e) => !gone && setError(String(e)),
    );
    return () => {
      gone = true;
    };
  }, [shown.id]);

  useEffect(() => {
    if (!md) return;
    const parts = ['Markdown', md.encoding];
    if (md.truncated) parts.push(t('view.truncated'));
    onMeta(parts.join(' · '));
  }, [md, onMeta, t]);

  const onClick = (e: MouseEvent) => {
    const a = (e.target as HTMLElement).closest('a');
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute('href') ?? '';
    if (href.startsWith('#')) {
      const id = ID_PREFIX + decodeURIComponent(href.slice(1));
      page.current?.querySelector(`[id="${CSS.escape(id)}"]`)?.scrollIntoView({ behavior: 'smooth' });
    } else if (/^(https?:|mailto:)/i.test(href)) {
      void api.openUrl(href);
    }
    // Links to other files: nothing (the window shows one file).
  };

  if (error) return <div className="look-note">{error}</div>;
  return (
    <div className="look-md-scroll">
      {/* The HTML is cleaned by ammonia in Rust before it gets here. */}
      <article ref={page} className="look-md" onClick={onClick} dangerouslySetInnerHTML={{ __html: md?.html ?? '' }} />
    </div>
  );
}
