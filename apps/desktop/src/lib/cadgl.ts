// A CAD drawing's lines and areas on the GPU (WebGL 2): uploaded once, then each frame is one
// draw call per colour, however many millions of points. Labels are drawn on a canvas above.

import { type CadDrawing, cssColor, rgbOf, segments, type View } from './cad';

const VERTEX = `#version 300 es
in vec2 p;
uniform vec4 m; // x scale, y scale, x offset, y offset (to clip space)
void main() { gl_Position = vec4(p * m.xy + m.zw, 0.0, 1.0); }`;

const FRAGMENT = `#version 300 es
precision mediump float;
uniform vec4 c;
out vec4 o;
void main() { o = c; }`;

interface Batch {
  buf: WebGLBuffer;
  /** Vertices. */
  n: number;
  color: number;
  alpha: number;
}

export interface GlScene {
  draw(v: View, w: number, h: number, dark: boolean, ground: [number, number, number] | null): void;
  dispose(): void;
}

function shader(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader | null {
  const s = gl.createShader(type);
  if (!s) return null;
  gl.shaderSource(s, src);
  gl.compileShader(s);
  return gl.getShaderParameter(s, gl.COMPILE_STATUS) ? s : null;
}

/** The scene on `canvas`, or null without WebGL 2. */
export function glScene(canvas: HTMLCanvasElement, d: CadDrawing): GlScene | null {
  const gl = canvas.getContext('webgl2', { antialias: true, alpha: true, premultipliedAlpha: true, preserveDrawingBuffer: false });
  if (!gl) return null;
  const vs = shader(gl, gl.VERTEX_SHADER, VERTEX);
  const fs = shader(gl, gl.FRAGMENT_SHADER, FRAGMENT);
  const prog = gl.createProgram();
  if (!vs || !fs || !prog) return null;
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return null;
  const at = gl.getAttribLocation(prog, 'p');
  const um = gl.getUniformLocation(prog, 'm');
  const uc = gl.getUniformLocation(prog, 'c');
  const vao = gl.createVertexArray();
  gl.bindVertexArray(vao);

  const upload = (data: Float32Array, color: number, alpha: number): Batch | null => {
    if (data.length < 4) return null;
    const buf = gl.createBuffer();
    if (!buf) return null;
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
    return { buf, n: data.length / 2, color, alpha };
  };
  const fills = d.fills.map((r) => upload(d.floats.subarray(r.start, r.end), r.color, r.alpha)).filter((b): b is Batch => !!b);
  const lines = d.strokes.map((r) => upload(segments(d.floats, r), r.color, 255)).filter((b): b is Batch => !!b);

  return {
    draw(v, w, h, dark, ground) {
      gl.viewport(0, 0, canvas.width, canvas.height);
      if (ground) gl.clearColor(ground[0], ground[1], ground[2], 1);
      else gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.useProgram(prog);
      gl.bindVertexArray(vao);
      gl.enableVertexAttribArray(at);
      // World (relative to the origin) → clip space; screen y runs down, world y up.
      gl.uniform4f(um, (2 * v.scale) / w, (2 * v.scale) / h, (2 * v.x) / w - 1, 1 - (2 * v.y) / h);
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
      const paint = (b: Batch, mode: number) => {
        const [r, g, bl] = rgbOf(cssColor(d.colors[b.color] ?? 0, dark));
        const a = b.alpha / 255;
        gl.uniform4f(uc, r * a, g * a, bl * a, a);
        gl.bindBuffer(gl.ARRAY_BUFFER, b.buf);
        gl.vertexAttribPointer(at, 2, gl.FLOAT, false, 0, 0);
        gl.drawArrays(mode, 0, b.n);
      };
      for (const b of fills) paint(b, gl.TRIANGLES);
      for (const b of lines) paint(b, gl.LINES);
    },
    dispose() {
      for (const b of [...fills, ...lines]) gl.deleteBuffer(b.buf);
      gl.deleteVertexArray(vao);
      gl.deleteProgram(prog);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
    },
  };
}
