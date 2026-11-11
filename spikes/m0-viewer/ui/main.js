// M0 spike page: renders whatever the shell sends and reports when it is on screen.
const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

const stage = document.getElementById('stage');
const nameEl = document.getElementById('name');
const metaEl = document.getElementById('meta');
let cur = null;
let t0 = 0;
window.__m0 = { last: null };

function done(what, detail) {
  if (!cur) return;
  const pageMs = performance.now() - t0;
  window.__m0.last = { id: cur.id, what, detail };
  invoke('painted', { id: cur.id, what, detail: JSON.stringify(detail ?? {}), pageMs });
}

// Paint, then report (two frames: the content is on screen).
const afterPaint = (fn) => requestAnimationFrame(() => requestAnimationFrame(fn));

function handlerRect() {
  const r = stage.getBoundingClientRect();
  const d = window.devicePixelRatio;
  return { x: Math.round(r.left * d), y: Math.round(r.top * d), w: Math.round(r.width * d), h: Math.round(r.height * d) };
}

function render(p) {
  cur = p;
  t0 = performance.now();
  nameEl.textContent = p.name;
  metaEl.textContent = `${p.kind} · ${p.size.toLocaleString()} bytes`;
  stage.replaceChildren();
  switch (p.kind) {
    case 'image': {
      const img = new Image();
      img.onload = () => afterPaint(() => done('image', { w: img.naturalWidth, h: img.naturalHeight }));
      img.onerror = () => done('image-error');
      img.src = p.url;
      stage.append(img);
      break;
    }
    case 'text': {
      fetch(p.url)
        .then((r) => r.text())
        .then((t) => {
          const pre = document.createElement('pre');
          pre.textContent = t.slice(0, 200000);
          stage.append(pre);
          afterPaint(() => done('text', { chars: t.length }));
        })
        .catch((e) => done('text-error', String(e)));
      break;
    }
    case 'pdf': {
      const f = document.createElement('iframe');
      f.onload = () => afterPaint(() => done('pdf-load'));
      f.src = p.url;
      stage.append(f);
      break;
    }
    case 'video':
    case 'audio': {
      const m = document.createElement(p.kind);
      m.controls = true;
      m.muted = true;
      m.autoplay = true;
      m.onloadeddata = () =>
        afterPaint(() => done(`${p.kind}-data`, { w: m.videoWidth, h: m.videoHeight, duration: m.duration }));
      m.onerror = () => done(`${p.kind}-error`, { code: m.error?.code, message: m.error?.message });
      m.src = p.url;
      stage.append(m);
      break;
    }
    case 'handler': {
      const div = document.createElement('div');
      div.className = 'handler';
      stage.append(div);
      afterPaint(() =>
        invoke('host_handler', { id: p.id, ...handlerRect() })
          .then((info) => done('handler', info))
          .catch((e) => done('handler-error', String(e))),
      );
      break;
    }
    default: {
      const pre = document.createElement('pre');
      pre.textContent = '(no previewer in the spike)';
      stage.append(pre);
      afterPaint(() => done('other'));
    }
  }
}

listen('preview', (e) => render(e.payload));
listen('clear', () => {
  cur = null;
  stage.replaceChildren();
  nameEl.textContent = '—';
  metaEl.textContent = '';
});

window.addEventListener('resize', () => {
  if (cur?.kind === 'handler') invoke('handler_rect', handlerRect());
});

window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' || (e.key === ' ' && e.ctrlKey)) {
    e.preventDefault();
    invoke('close_preview');
  }
});

// What this WebView2 says it can play.
const types = {
  'mp4 h264/aac': 'video/mp4; codecs="avc1.42E01E, mp4a.40.2"',
  'mp4 hevc': 'video/mp4; codecs="hvc1.1.6.L93.B0"',
  'mp4 av1': 'video/mp4; codecs="av01.0.05M.08"',
  'webm vp9/opus': 'video/webm; codecs="vp9, opus"',
  'webm av1': 'video/webm; codecs="av01.0.05M.08"',
  'mkv': 'video/x-matroska',
  'mov': 'video/quicktime',
  'avi': 'video/x-msvideo',
  'wmv': 'video/x-ms-wmv',
  'ogg theora': 'video/ogg; codecs="theora"',
  'mp3': 'audio/mpeg',
  'aac': 'audio/aac',
  'm4a': 'audio/mp4; codecs="mp4a.40.2"',
  'flac': 'audio/flac',
  'wav': 'audio/wav',
  'ogg vorbis': 'audio/ogg; codecs="vorbis"',
  'opus': 'audio/ogg; codecs="opus"',
  'wma': 'audio/x-ms-wma',
};
const probe = document.createElement('video');
const can = Object.entries(types).map(([k, t]) => `${k}=${probe.canPlayType(t) || 'no'}`);
invoke('page_log', { line: `ready after ${performance.now().toFixed(0)} ms; canPlayType ${can.join(', ')}` });
invoke('page_log', { line: navigator.userAgent });
