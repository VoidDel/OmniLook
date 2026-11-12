//! CAD drawings (DXF, DWG): read with acadrust and drawn here, as seen from above, into what the
//! page's canvas needs: lines by colour, filled areas and labels. Blocks are expanded, colours
//! resolved (by layer, by block), hidden layers left out. The page zooms and pans without asking
//! again.
//!
//! The geometry goes to the page as binary (`Drawing::encode`), not JSON: a large plan is
//! millions of points.

use std::{collections::HashMap, f64::consts::TAU, fs::File, io::Read, path::Path};

use acadrust::{
    CadDocument, Color, DwgReadOptions, DwgReader, DxfReader, DxfReaderConfiguration, DxfVersion,
    EntityType,
    entities::{
        BoundaryEdge, BoundaryPath, Hatch, Spline, TextHorizontalAlignment, TextVerticalAlignment,
        attribute_definition::{HorizontalAlignment, VerticalAlignment},
    },
    types::{Vector2, Vector3, aci_table::aci_to_rgb},
};
use serde::Serialize;

/// Drawings this module reads.
pub fn handles(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| e == "dxf" || e == "dwg")
}

/// The colour that is white on dark and black on light (ACI 7, and what has no colour).
pub const FOREGROUND: u32 = 0x0100_0000;

/// Points drawn at most; a drawing with more is cut short (`truncated`).
const MAX_POINTS: usize = 4_000_000;
/// Labels drawn at most.
const MAX_LABELS: usize = 100_000;
/// Entities visited at most, block contents counted each time they are inserted.
const MAX_VISITS: usize = 3_000_000;
/// How deep blocks may sit in blocks.
const MAX_DEPTH: u8 = 16;
/// Pattern hatches with more lines than this are shown as a light fill instead.
const MAX_HATCH_LINES: f64 = 20_000.0;

/// Lines of one colour: a run of `[n, x1, y1, … xn, yn]` in `Drawing::floats`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    /// Index into `colors`.
    pub color: u16,
    /// Fills: 0–255.
    pub alpha: u8,
    /// `[start, end)` in floats.
    pub start: u32,
    pub end: u32,
}

/// A line of text. `x`/`y` is where `align` puts it; `xa` is the text's direction and `ya` its
/// up, both as long as the text is high (the width factor and slant are in them).
#[derive(Debug, Clone, Serialize)]
pub struct Label {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub xa: [f32; 2],
    pub ya: [f32; 2],
    /// 0 left, 1 center, 2 right.
    pub h: u8,
    /// 0 baseline, 1 bottom, 2 middle, 3 top.
    pub v: u8,
    pub color: u16,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drawing {
    /// "DWG" or "DXF".
    pub format: &'static str,
    /// "AutoCAD 2013".
    pub version: Option<&'static str>,
    /// Drawing units, "mm".
    pub units: Option<&'static str>,
    /// Entities drawn (in model space, a block insert as one).
    pub objects: usize,
    pub layers: usize,
    /// Paper space, the model being empty.
    pub paper: bool,
    /// Where the coordinates are from: they are sent relative to it, as f32.
    pub origin: [f64; 2],
    /// `[min x, min y, max x, max y]`, relative to `origin`.
    pub bounds: Option<[f32; 4]>,
    /// 0xRRGGBB, or `FOREGROUND`.
    pub colors: Vec<u32>,
    /// Lines: `[n, points…]` repeated.
    pub strokes: Vec<Run>,
    /// Areas, as triangles: `[x1, y1, x2, y2, x3, y3]` repeated.
    pub fills: Vec<Run>,
    pub labels: Vec<Label>,
    /// Too large to draw all of it.
    pub truncated: bool,
    #[serde(skip)]
    pub floats: Vec<f32>,
}

impl Drawing {
    /// `[json length: u32 LE][json][padding to 4][floats, f32 LE]`.
    pub fn encode(&self) -> Vec<u8> {
        let json = serde_json::to_vec(self).unwrap_or_default();
        let pad = (4 - (4 + json.len()) % 4) % 4;
        let mut out = Vec::with_capacity(4 + json.len() + pad + self.floats.len() * 4);
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&json);
        out.resize(out.len() + pad, b' ');
        for f in &self.floats {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out
    }
}

fn is_dwg(path: &Path) -> bool {
    let mut head = [0u8; 6];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && head.starts_with(b"AC1")
}

fn version_name(v: DxfVersion) -> Option<&'static str> {
    Some(match v {
        DxfVersion::AC1009 => "AutoCAD R12",
        DxfVersion::AC1012 => "AutoCAD R13",
        DxfVersion::AC1014 => "AutoCAD R14",
        DxfVersion::AC1015 => "AutoCAD 2000",
        DxfVersion::AC1018 => "AutoCAD 2004",
        DxfVersion::AC1021 => "AutoCAD 2007",
        DxfVersion::AC1024 => "AutoCAD 2010",
        DxfVersion::AC1027 => "AutoCAD 2013",
        DxfVersion::AC1032 => "AutoCAD 2018",
        _ => return None,
    })
}

fn units_name(u: i16) -> Option<&'static str> {
    Some(match u {
        1 => "in",
        2 => "ft",
        3 => "mi",
        4 => "mm",
        5 => "cm",
        6 => "m",
        7 => "km",
        8 => "µin",
        9 => "mil",
        10 => "yd",
        13 => "µm",
        14 => "dm",
        _ => return None,
    })
}

pub fn read(path: &Path) -> Result<Drawing, String> {
    let dwg = is_dwg(path);
    let doc = if dwg {
        let options = DwgReadOptions {
            failsafe: true,
            ..Default::default()
        };
        DwgReader::from_file_with_options(path, options)
            .and_then(|mut r| r.read())
            .map_err(|e| e.to_string())?
    } else {
        DxfReader::from_file(path)
            .map(|r| {
                r.with_configuration(DxfReaderConfiguration {
                    failsafe: true,
                    ..Default::default()
                })
            })
            .and_then(|r| r.read())
            .map_err(|e| e.to_string())?
    };
    Ok(draw(&doc, if dwg { "DWG" } else { "DXF" }))
}

/// The drawing seen from above.
pub fn draw(doc: &CadDocument, format: &'static str) -> Drawing {
    let mut p = Painter::new(doc);
    let inherit = Inherit {
        color: FOREGROUND,
        layer: "0",
    };
    let mut objects = 0;
    let mut paper = false;
    for (i, space) in ["*Model_Space", "*Paper_Space"].into_iter().enumerate() {
        for e in doc.entities_in_block(space) {
            if p.full {
                break;
            }
            p.object = (
                (f64::INFINITY, f64::INFINITY),
                (f64::NEG_INFINITY, f64::NEG_INFINITY),
            );
            p.entity(e, &Xf::ID, &inherit, 0);
            let (lo, hi) = p.object;
            if lo.0 <= hi.0 && lo.1 <= hi.1 {
                p.boxes.push([lo.0, lo.1, hi.0, hi.1]);
            }
            objects += 1;
        }
        if p.points > 0 || !p.labels.is_empty() {
            paper = i == 1;
            break;
        }
        objects = 0;
    }
    p.finish(Drawing {
        format,
        version: version_name(doc.version),
        units: units_name(doc.header.insertion_units),
        objects,
        layers: doc.layers.iter().count(),
        paper,
        origin: [0.0; 2],
        bounds: None,
        colors: Vec::new(),
        strokes: Vec::new(),
        fills: Vec::new(),
        labels: Vec::new(),
        truncated: false,
        floats: Vec::new(),
    })
}

// --- geometry ---------------------------------------------------------------------------------

/// A 2D affine map: `x' = a x + c y + e`, `y' = b x + d y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Xf {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Xf {
    const ID: Xf = Xf {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn at(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    fn vec(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }

    /// `self` after `inner`.
    fn then(&self, inner: &Xf) -> Xf {
        let (a, b) = self.vec(inner.a, inner.b);
        let (c, d) = self.vec(inner.c, inner.d);
        let (e, f) = self.at(inner.e, inner.f);
        Xf { a, b, c, d, e, f }
    }

    fn translate(x: f64, y: f64) -> Xf {
        Xf {
            e: x,
            f: y,
            ..Xf::ID
        }
    }

    fn rotate(r: f64) -> Xf {
        let (s, c) = r.sin_cos();
        Xf {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        }
    }

    fn scale(x: f64, y: f64) -> Xf {
        Xf {
            a: x,
            d: y,
            ..Xf::ID
        }
    }

    /// An entity's own coordinate system (the arbitrary axis algorithm), at height `z`, seen
    /// from above.
    fn ocs(n: &Vector3, z: f64) -> Xf {
        let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
        if len.is_nan() || len <= 1e-12 || (n.x.abs() < 1e-12 && n.y.abs() < 1e-12 && n.z > 0.0) {
            return Xf::translate(0.0, 0.0);
        }
        let n = [n.x / len, n.y / len, n.z / len];
        let cross = |u: [f64; 3], v: [f64; 3]| {
            [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]
        };
        let unit = |v: [f64; 3]| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-300);
            [v[0] / l, v[1] / l, v[2] / l]
        };
        let ax = if n[0].abs() < 1.0 / 64.0 && n[1].abs() < 1.0 / 64.0 {
            unit(cross([0.0, 1.0, 0.0], n))
        } else {
            unit(cross([0.0, 0.0, 1.0], n))
        };
        let ay = unit(cross(n, ax));
        Xf {
            a: ax[0],
            b: ax[1],
            c: ay[0],
            d: ay[1],
            e: z * n[0],
            f: z * n[1],
        }
    }
}

/// Segments for a sweep of `angle` radians.
fn segments(angle: f64) -> usize {
    ((angle.abs() / TAU * 96.0).ceil() as usize).clamp(2, 256)
}

/// Points of the arc around `(cx, cy)` from `start`, turning by `sweep` (negative: clockwise).
fn arc_points(out: &mut Vec<(f64, f64)>, cx: f64, cy: f64, r: f64, start: f64, sweep: f64) {
    let n = segments(sweep);
    for i in 0..=n {
        let a = start + sweep * i as f64 / n as f64;
        out.push((cx + r * a.cos(), cy + r * a.sin()));
    }
}

/// The counter-clockwise sweep from `start` to `end`, a full turn when they meet.
fn ccw_sweep(start: f64, end: f64) -> f64 {
    let s = (end - start).rem_euclid(TAU);
    if s < 1e-9 { TAU } else { s }
}

/// The points from `a` to `b` along a polyline segment with `bulge` (`b` left out).
fn bulge_points(out: &mut Vec<(f64, f64)>, a: (f64, f64), b: (f64, f64), bulge: f64) {
    if bulge.abs() < 1e-9 || !bulge.is_finite() {
        out.push(a);
        return;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let chord = (dx * dx + dy * dy).sqrt();
    if chord < 1e-12 {
        out.push(a);
        return;
    }
    let sweep = 4.0 * bulge.atan();
    let r = chord / (2.0 * (sweep / 2.0).sin()).abs();
    // The centre: from the chord's middle, at distance r·cos(sweep/2) to its left (bulge > 0).
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let h = r * (sweep / 2.0).cos() * sweep.signum();
    let (cx, cy) = (mx - dy / chord * h, my + dx / chord * h);
    let start = (a.1 - cy).atan2(a.0 - cx);
    let mut pts = Vec::new();
    arc_points(&mut pts, cx, cy, r, start, sweep);
    pts.pop();
    out.extend(pts);
}

/// A NURBS curve, by de Boor's algorithm; `None` when the knots do not fit the points.
fn nurbs(
    degree: usize,
    points: &[(f64, f64)],
    weights: &[f64],
    knots: &[f64],
) -> Option<Vec<(f64, f64)>> {
    let n = points.len();
    let p = degree;
    if p == 0 || n <= p || knots.len() != n + p + 1 || knots.windows(2).any(|w| w[1] < w[0]) {
        return None;
    }
    let w = |i: usize| weights.get(i).copied().filter(|w| *w > 0.0).unwrap_or(1.0);
    let at = |u: f64, k: usize| {
        let mut d: Vec<[f64; 3]> = (0..=p)
            .map(|j| {
                let (x, y) = points[j + k - p];
                let wj = w(j + k - p);
                [x * wj, y * wj, wj]
            })
            .collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = j + k - p;
                let den = knots[i + p + 1 - r] - knots[i];
                let t = if den.abs() < 1e-300 {
                    0.0
                } else {
                    (u - knots[i]) / den
                };
                let prev = d[j - 1];
                for (c, p) in d[j].iter_mut().zip(prev) {
                    *c = (1.0 - t) * p + t * *c;
                }
            }
        }
        let wd = if d[p][2].abs() < 1e-300 { 1.0 } else { d[p][2] };
        (d[p][0] / wd, d[p][1] / wd)
    };
    let spans = (p..n).filter(|&k| knots[k + 1] > knots[k]).count().max(1);
    // More points per span for a short curve, fewer for a long one.
    let per = (512 / spans).clamp(4, 16);
    let mut out = Vec::new();
    for k in p..n {
        let (u0, u1) = (knots[k], knots[k + 1]);
        if u1 <= u0 {
            continue;
        }
        let first = if out.is_empty() { 0 } else { 1 };
        for i in first..=per {
            out.push(at(u0 + (u1 - u0) * i as f64 / per as f64, k));
        }
    }
    (out.len() >= 2).then_some(out)
}

fn spline_points(s: &Spline) -> Vec<(f64, f64)> {
    let cps: Vec<(f64, f64)> = s.control_points.iter().map(|v| (v.x, v.y)).collect();
    let degree = usize::try_from(s.degree).unwrap_or(3);
    if let Some(pts) = nurbs(degree, &cps, &s.weights, &s.knots) {
        return pts;
    }
    if s.fit_points.len() >= 2 {
        return s.fit_points.iter().map(|v| (v.x, v.y)).collect();
    }
    cps
}

/// A hatch loop as points, in the hatch's own coordinates.
fn loop_points(path: &BoundaryPath) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    let push = |out: &mut Vec<(f64, f64)>, pts: Vec<(f64, f64)>| {
        for p in pts {
            if out
                .last()
                .is_none_or(|q| (q.0 - p.0).abs() > 1e-9 || (q.1 - p.1).abs() > 1e-9)
            {
                out.push(p);
            }
        }
    };
    for edge in &path.edges {
        let mut pts = Vec::new();
        match edge {
            BoundaryEdge::Line(e) => pts.extend([(e.start.x, e.start.y), (e.end.x, e.end.y)]),
            BoundaryEdge::CircularArc(e) => {
                // Clockwise edges keep their angles and run the other way.
                let sweep = if e.counter_clockwise {
                    ccw_sweep(e.start_angle, e.end_angle)
                } else {
                    -ccw_sweep(e.end_angle, e.start_angle)
                };
                arc_points(
                    &mut pts,
                    e.center.x,
                    e.center.y,
                    e.radius,
                    e.start_angle,
                    sweep,
                );
            }
            BoundaryEdge::EllipticArc(e) => {
                let sweep = if e.counter_clockwise {
                    ccw_sweep(e.start_angle, e.end_angle)
                } else {
                    -ccw_sweep(e.end_angle, e.start_angle)
                };
                let m = e.major_axis_endpoint;
                let (mx, my) = (m.x, m.y);
                let (nx, ny) = (-my * e.minor_axis_ratio, mx * e.minor_axis_ratio);
                let n = segments(sweep);
                for i in 0..=n {
                    let t = e.start_angle + sweep * i as f64 / n as f64;
                    let (c, s) = (t.cos(), t.sin());
                    pts.push((e.center.x + mx * c + nx * s, e.center.y + my * c + ny * s));
                }
            }
            BoundaryEdge::Spline(e) => {
                let cps: Vec<(f64, f64)> = e.control_points.iter().map(|v| (v.x, v.y)).collect();
                let weights: Vec<f64> = if e.rational {
                    e.control_points.iter().map(|v| v.z).collect()
                } else {
                    Vec::new()
                };
                let degree = usize::try_from(e.degree).unwrap_or(3);
                pts = nurbs(degree, &cps, &weights, &e.knots).unwrap_or_else(|| {
                    if e.fit_points.len() >= 2 {
                        e.fit_points.iter().map(|v: &Vector2| (v.x, v.y)).collect()
                    } else {
                        cps
                    }
                });
            }
            BoundaryEdge::Polyline(e) => {
                let v = &e.vertices;
                let count = if e.is_closed {
                    v.len()
                } else {
                    v.len().saturating_sub(1)
                };
                for i in 0..count {
                    let (a, b) = (&v[i], &v[(i + 1) % v.len()]);
                    // The bulge is in z.
                    bulge_points(&mut pts, (a.x, a.y), (b.x, b.y), a.z);
                }
                if let Some(last) = if e.is_closed { v.first() } else { v.last() } {
                    pts.push((last.x, last.y));
                }
            }
        }
        push(&mut out, pts);
    }
    out
}

/// `%%c`, `%%d`, `%%p` and the like in TEXT: the characters they stand for.
fn special_codes(s: &str) -> String {
    if !s.contains("%%") {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("%%") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let mut chars = after.chars();
        match chars.next().map(|c| c.to_ascii_lowercase()) {
            Some('c') => out.push('⌀'),
            Some('d') => out.push('°'),
            Some('p') => out.push('±'),
            Some('%') => out.push('%'),
            // Underline, overline, strike-through: toggled, not drawn.
            Some('u' | 'o' | 'k') => {}
            Some(c) if c.is_ascii_digit() => {
                let digits: String = after
                    .chars()
                    .take(3)
                    .take_while(char::is_ascii_digit)
                    .collect();
                if let Some(ch) = digits.parse::<u32>().ok().and_then(char::from_u32) {
                    out.push(ch);
                }
                rest = &after[digits.len()..];
                continue;
            }
            Some(_) => out.push_str("%%"),
            None => {
                out.push_str("%%");
                rest = "";
                continue;
            }
        }
        rest = &after[after.chars().next().map_or(0, char::len_utf8)..];
    }
    out.push_str(rest);
    out
}

/// Whether `p` is inside `ring` (even-odd).
fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut odd = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            odd = !odd;
        }
        j = i;
    }
    odd
}

fn area(ring: &[[f64; 2]]) -> f64 {
    let mut s = 0.0;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        s += (ring[j][0] - ring[i][0]) * (ring[j][1] + ring[i][1]);
        j = i;
    }
    (s / 2.0).abs()
}

/// Rings filled even-odd, as triangles (three points each). A ring inside an odd number of
/// others is a hole in the smallest of them that is not itself a hole.
fn triangulate(rings: &[Vec<[f64; 2]>]) -> Vec<[f64; 2]> {
    // Many rings (a hatch around lettering): each its own area, without telling holes.
    let depth: Vec<usize> = if rings.len() > 500 {
        vec![0; rings.len()]
    } else {
        rings
            .iter()
            .enumerate()
            .map(|(i, r)| {
                (0..rings.len())
                    .filter(|&j| j != i && inside(r[0], &rings[j]))
                    .count()
            })
            .collect()
    };
    let areas: Vec<f64> = rings.iter().map(|r| area(r)).collect();
    let mut holes: Vec<Vec<usize>> = vec![Vec::new(); rings.len()];
    for (i, r) in rings.iter().enumerate() {
        if depth[i] % 2 == 1
            && let Some(outer) = (0..rings.len())
                .filter(|&j| depth[j] + 1 == depth[i] && inside(r[0], &rings[j]))
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
        {
            holes[outer].push(i);
        }
    }
    let mut out = Vec::new();
    let mut cut = earcut::Earcut::new();
    let mut data: Vec<[f64; 2]> = Vec::new();
    let mut starts: Vec<u32> = Vec::new();
    let mut tris: Vec<u32> = Vec::new();
    for (i, r) in rings.iter().enumerate() {
        if depth[i] % 2 == 1 {
            continue;
        }
        data.clear();
        starts.clear();
        data.extend_from_slice(r);
        for &h in &holes[i] {
            starts.push(data.len() as u32);
            data.extend_from_slice(&rings[h]);
        }
        cut.earcut(data.iter().copied(), &starts, &mut tris);
        out.extend(tris.iter().map(|&k| data[k as usize]));
    }
    out
}

/// What the view fits: the extent of the entities, less the odd one far away from the rest (a
/// stray point, a block inserted at a wild scale) that would shrink the drawing to a dot. `None`
/// when there are too few to tell.
fn extents(boxes: &[[f64; 4]]) -> Option<[f64; 4]> {
    if boxes.len() < 8 {
        return None;
    }
    let pick = |f: &dyn Fn(&[f64; 4]) -> f64| {
        let mut v: Vec<f64> = boxes.iter().map(f).collect();
        v.sort_by(f64::total_cmp);
        let n = v.len();
        (v[n * 2 / 100], v[(n * 98).div_ceil(100) - 1])
    };
    // Where most entities are…
    let (x0, x1) = pick(&|b| (b[0] + b[2]) / 2.0);
    let (y0, y1) = pick(&|b| (b[1] + b[3]) / 2.0);
    let w = (x1 - x0).max(y1 - y0);
    let w = if w > 0.0 {
        w
    } else {
        boxes
            .iter()
            .map(|b| (b[2] - b[0]).max(b[3] - b[1]))
            .fold(0.0, f64::max)
    };
    // …and what is near them and not much larger.
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for b in boxes {
        let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        let near = cx >= x0 - w && cx <= x1 + w && cy >= y0 - w && cy <= y1 + w;
        if near && (b[2] - b[0]).max(b[3] - b[1]) <= 4.0 * w {
            out = [
                out[0].min(b[0]),
                out[1].min(b[1]),
                out[2].max(b[2]),
                out[3].max(b[3]),
            ];
        }
    }
    (out[0] <= out[2]).then_some(out)
}

// --- painting ---------------------------------------------------------------------------------

/// What an entity in a block takes from the insert: its colour (`ByBlock`) and, on layer 0, its
/// layer.
#[derive(Clone, Copy)]
struct Inherit<'a> {
    color: u32,
    layer: &'a str,
}

struct LayerLook {
    color: u32,
    visible: bool,
}

struct Painter<'a> {
    doc: &'a CadDocument,
    layers: HashMap<String, LayerLook>,
    colors: Vec<u32>,
    color_ix: HashMap<u32, u16>,
    strokes: HashMap<u16, Vec<f32>>,
    fills: HashMap<(u16, u8), Vec<f32>>,
    labels: Vec<Label>,
    origin: Option<(f64, f64)>,
    min: (f64, f64),
    max: (f64, f64),
    /// The extent of the top-level entity being drawn, then of each (`boxes`).
    object: ((f64, f64), (f64, f64)),
    boxes: Vec<[f64; 4]>,
    points: usize,
    visits: usize,
    full: bool,
    pts: Vec<(f64, f64)>,
}

fn rgb_of(c: &Color) -> Option<u32> {
    match c {
        Color::Index(7) => Some(FOREGROUND),
        Color::Index(i) => aci_to_rgb(*i).map(|(r, g, b)| u32::from_be_bytes([0, r, g, b])),
        Color::Rgb { r, g, b } => Some(u32::from_be_bytes([0, *r, *g, *b])),
        _ => None,
    }
}

impl<'a> Painter<'a> {
    fn new(doc: &'a CadDocument) -> Self {
        let layers = doc
            .layers
            .iter()
            .map(|l| {
                let look = LayerLook {
                    color: rgb_of(&l.color).unwrap_or(FOREGROUND),
                    visible: !l.flags.off && !l.flags.frozen,
                };
                (l.name.to_lowercase(), look)
            })
            .collect();
        Painter {
            doc,
            layers,
            colors: Vec::new(),
            color_ix: HashMap::new(),
            strokes: HashMap::new(),
            fills: HashMap::new(),
            labels: Vec::new(),
            origin: None,
            min: (f64::INFINITY, f64::INFINITY),
            max: (f64::NEG_INFINITY, f64::NEG_INFINITY),
            object: (
                (f64::INFINITY, f64::INFINITY),
                (f64::NEG_INFINITY, f64::NEG_INFINITY),
            ),
            boxes: Vec::new(),
            points: 0,
            visits: 0,
            full: false,
            pts: Vec::new(),
        }
    }

    fn color_index(&mut self, rgb: u32) -> u16 {
        if let Some(i) = self.color_ix.get(&rgb) {
            return *i;
        }
        let i = self.colors.len() as u16;
        self.colors.push(rgb);
        self.color_ix.insert(rgb, i);
        i
    }

    /// A point, relative to the origin (the first point drawn), counted in the bounds.
    fn rel(&mut self, (x, y): (f64, f64)) -> (f32, f32) {
        let o = *self.origin.get_or_insert((x, y));
        self.min = (self.min.0.min(x), self.min.1.min(y));
        self.max = (self.max.0.max(x), self.max.1.max(y));
        let (lo, hi) = &mut self.object;
        *lo = (lo.0.min(x), lo.1.min(y));
        *hi = (hi.0.max(x), hi.1.max(y));
        ((x - o.0) as f32, (y - o.1) as f32)
    }

    fn room(&mut self, n: usize) -> bool {
        if self.points + n > MAX_POINTS {
            self.full = true;
        }
        !self.full
    }

    /// `self.pts` through `xf` as a line.
    fn stroke(&mut self, xf: &Xf, color: u32, closed: bool) {
        let mut pts = std::mem::take(&mut self.pts);
        pts.retain(|p| p.0.is_finite() && p.1.is_finite());
        if closed && pts.len() > 2 {
            pts.push(pts[0]);
        }
        if pts.len() >= 2 && self.room(pts.len()) {
            let ix = self.color_index(color);
            let mut buf = self.strokes.remove(&ix).unwrap_or_default();
            buf.push(pts.len() as f32);
            for p in &pts {
                let (x, y) = self.rel(xf.at(p.0, p.1));
                buf.extend([x, y]);
            }
            self.points += pts.len();
            self.strokes.insert(ix, buf);
        }
        pts.clear();
        self.pts = pts;
    }

    /// Rings through `xf` as one area (filled even-odd), cut into triangles.
    fn fill(&mut self, xf: &Xf, color: u32, alpha: u8, rings: &[Vec<(f64, f64)>]) {
        let rings: Vec<Vec<[f64; 2]>> = rings
            .iter()
            .filter(|r| r.len() >= 3 && r.iter().all(|p| p.0.is_finite() && p.1.is_finite()))
            .map(|r| {
                r.iter()
                    .map(|p| {
                        let (x, y) = xf.at(p.0, p.1);
                        [x, y]
                    })
                    .collect()
            })
            .collect();
        let n: usize = rings.iter().map(Vec::len).sum();
        if rings.is_empty() || !self.room(n * 3) {
            return;
        }
        let triangles = triangulate(&rings);
        self.triangles(color, alpha, &triangles);
    }

    /// Triangles (three points each, already in place) of one colour.
    fn triangles(&mut self, color: u32, alpha: u8, triangles: &[[f64; 2]]) {
        if triangles.is_empty()
            || triangles
                .iter()
                .any(|p| !p[0].is_finite() || !p[1].is_finite())
            || !self.room(triangles.len())
        {
            return;
        }
        let ix = self.color_index(color);
        let mut buf = self.fills.remove(&(ix, alpha)).unwrap_or_default();
        for p in triangles {
            let (x, y) = self.rel((p[0], p[1]));
            buf.extend([x, y]);
        }
        self.points += triangles.len();
        self.fills.insert((ix, alpha), buf);
    }

    #[allow(clippy::too_many_arguments)]
    fn label(
        &mut self,
        text: &str,
        at: (f64, f64),
        xf: &Xf,
        height: f64,
        rotation: f64,
        width: f64,
        oblique: f64,
        (h, v): (u8, u8),
        mirror: (bool, bool),
        color: u32,
    ) {
        let text = special_codes(text);
        if text.trim().is_empty() || !height.is_finite() || height <= 0.0 {
            return;
        }
        if self.labels.len() >= MAX_LABELS {
            self.full = true;
            return;
        }
        let width = if width > 0.0 && width.is_finite() {
            width
        } else {
            1.0
        };
        let (s, c) = rotation.sin_cos();
        let mut dir = (c * height * width, s * height * width);
        let slant = if oblique.is_finite() {
            oblique.tan().clamp(-5.0, 5.0)
        } else {
            0.0
        };
        let mut up = (
            -s * height + c * height * slant,
            c * height + s * height * slant,
        );
        if mirror.0 {
            dir = (-dir.0, -dir.1);
        }
        if mirror.1 {
            up = (-up.0, -up.1);
        }
        let xa = xf.vec(dir.0, dir.1);
        let ya = xf.vec(up.0, up.1);
        let (x, y) = self.rel(xf.at(at.0, at.1));
        let color = self.color_index(color);
        self.labels.push(Label {
            text,
            x,
            y,
            xa: [xa.0 as f32, xa.1 as f32],
            ya: [ya.0 as f32, ya.1 as f32],
            h,
            v,
            color,
        });
    }

    fn entity(&mut self, e: &EntityType, xf: &Xf, inherit: &Inherit, depth: u8) {
        self.visits += 1;
        if self.visits > MAX_VISITS {
            self.full = true;
        }
        if self.full {
            return;
        }
        let common = e.common();
        if common.invisible {
            return;
        }
        let layer: &str = if depth > 0 && common.layer == "0" {
            inherit.layer
        } else {
            &common.layer
        };
        let look = self.layers.get(&layer.to_lowercase());
        if look.is_some_and(|l| !l.visible) {
            return;
        }
        let by_layer = look.map_or(FOREGROUND, |l| l.color);
        let color = match &common.color {
            Color::ByLayer => by_layer,
            Color::ByBlock => inherit.color,
            c => rgb_of(c).unwrap_or(FOREGROUND),
        };
        let mut pts = std::mem::take(&mut self.pts);
        pts.clear();
        match e {
            EntityType::Line(l) => {
                pts.extend([(l.start.x, l.start.y), (l.end.x, l.end.y)]);
                self.pts = pts;
                self.stroke(xf, color, false);
            }
            EntityType::Circle(c) => {
                let o = xf.then(&Xf::ocs(&c.normal, c.center.z));
                arc_points(&mut pts, c.center.x, c.center.y, c.radius, 0.0, TAU);
                self.pts = pts;
                self.stroke(&o, color, false);
            }
            EntityType::Arc(a) => {
                let o = xf.then(&Xf::ocs(&a.normal, a.center.z));
                let sweep = ccw_sweep(a.start_angle, a.end_angle);
                arc_points(
                    &mut pts,
                    a.center.x,
                    a.center.y,
                    a.radius,
                    a.start_angle,
                    sweep,
                );
                self.pts = pts;
                self.stroke(&o, color, false);
            }
            EntityType::Ellipse(el) => {
                let m = &el.major_axis;
                let n = &el.normal;
                // The minor axis: the normal × the major one, scaled.
                let r = el.minor_axis_ratio;
                let minor = ((n.y * m.z - n.z * m.y) * r, (n.z * m.x - n.x * m.z) * r);
                let (s0, s1) = (el.start_parameter, el.end_parameter);
                let sweep = if (s1 - s0 - TAU).abs() < 1e-9 || (s1 - s0).abs() < 1e-12 {
                    TAU
                } else {
                    ccw_sweep(s0, s1)
                };
                let k = segments(sweep);
                for i in 0..=k {
                    let t = s0 + sweep * i as f64 / k as f64;
                    let (c, s) = (t.cos(), t.sin());
                    pts.push((
                        el.center.x + m.x * c + minor.0 * s,
                        el.center.y + m.y * c + minor.1 * s,
                    ));
                }
                self.pts = pts;
                self.stroke(xf, color, false);
            }
            EntityType::LwPolyline(pl) => {
                let o = xf.then(&Xf::ocs(&pl.normal, pl.elevation));
                let v = &pl.vertices;
                let count = if pl.is_closed {
                    v.len()
                } else {
                    v.len().saturating_sub(1)
                };
                for i in 0..count {
                    let (a, b) = (&v[i], &v[(i + 1) % v.len()]);
                    bulge_points(
                        &mut pts,
                        (a.location.x, a.location.y),
                        (b.location.x, b.location.y),
                        a.bulge,
                    );
                }
                if let Some(last) = if pl.is_closed { v.first() } else { v.last() } {
                    pts.push((last.location.x, last.location.y));
                }
                self.pts = pts;
                self.stroke(&o, color, false);
            }
            EntityType::Polyline2D(pl) => {
                let bits = pl.flags.bits();
                let closed = bits & 1 != 0;
                let o = xf.then(&Xf::ocs(&pl.normal, pl.elevation));
                // Fitted curves list their frame (control) points too: those are not drawn.
                let v: Vec<_> = pl
                    .vertices
                    .iter()
                    .filter(|v| v.flags.bits() & 16 == 0)
                    .collect();
                let count = if closed {
                    v.len()
                } else {
                    v.len().saturating_sub(1)
                };
                for i in 0..count {
                    let (a, b) = (v[i], v[(i + 1) % v.len()]);
                    bulge_points(
                        &mut pts,
                        (a.location.x, a.location.y),
                        (b.location.x, b.location.y),
                        a.bulge,
                    );
                }
                if let Some(last) = if closed { v.first() } else { v.last() } {
                    pts.push((last.location.x, last.location.y));
                }
                self.pts = pts;
                self.stroke(&o, color, false);
            }
            EntityType::Polyline(pl) if pl.flags.bits() & (16 | 64) == 0 => {
                pts.extend(
                    pl.vertices
                        .iter()
                        .filter(|v| v.flags.bits() & 16 == 0)
                        .map(|v| (v.location.x, v.location.y)),
                );
                self.pts = pts;
                self.stroke(xf, color, pl.flags.bits() & 1 != 0);
            }
            EntityType::Polyline3D(pl) if !pl.flags.is_3d_mesh && !pl.flags.is_polyface_mesh => {
                pts.extend(
                    pl.vertices
                        .iter()
                        .filter(|v| v.flags & 16 == 0)
                        .map(|v| (v.position.x, v.position.y)),
                );
                self.pts = pts;
                self.stroke(xf, color, pl.flags.closed);
            }
            EntityType::Spline(s) => {
                self.pts = spline_points(s);
                self.stroke(xf, color, false);
            }
            EntityType::Solid(s) => {
                // Two triangles, 1-2-3 and 2-4-3, as CAD fills it: crossed corners make a bow tie.
                let o = xf.then(&Xf::ocs(&s.normal, s.first_corner.z));
                let at = |v: &Vector3| {
                    let (x, y) = o.at(v.x, v.y);
                    [x, y]
                };
                let (a, b, c, d) = (
                    at(&s.first_corner),
                    at(&s.second_corner),
                    at(&s.third_corner),
                    at(&s.fourth_corner),
                );
                self.triangles(color, 255, &[a, b, c, b, d, c]);
            }
            EntityType::Face3D(f) => {
                let c = [
                    &f.first_corner,
                    &f.second_corner,
                    &f.third_corner,
                    &f.fourth_corner,
                ];
                pts.extend(c.iter().map(|v| (v.x, v.y)));
                self.pts = pts;
                self.stroke(xf, color, true);
            }
            EntityType::Text(t) => {
                let o = xf.then(&Xf::ocs(&t.normal, t.insertion_point.z));
                let (h, v) = (
                    match t.horizontal_alignment {
                        TextHorizontalAlignment::Left => 0,
                        TextHorizontalAlignment::Right => 2,
                        _ => 1,
                    },
                    match t.vertical_alignment {
                        TextVerticalAlignment::Baseline => 0,
                        TextVerticalAlignment::Bottom => 1,
                        TextVerticalAlignment::Middle => 2,
                        TextVerticalAlignment::Top => 3,
                    },
                );
                let fit = matches!(
                    t.horizontal_alignment,
                    TextHorizontalAlignment::Aligned | TextHorizontalAlignment::Fit
                );
                let v = if t.horizontal_alignment == TextHorizontalAlignment::Middle {
                    2
                } else {
                    v
                };
                let ins = (t.insertion_point.x, t.insertion_point.y);
                let at = match t.alignment_point {
                    Some(p) if fit => ((ins.0 + p.x) / 2.0, (ins.1 + p.y) / 2.0),
                    Some(p) if (h, v) != (0, 0) => (p.x, p.y),
                    _ => ins,
                };
                let flags = t.generation_flags;
                self.label(
                    &t.value,
                    at,
                    &o,
                    t.height,
                    t.rotation,
                    t.width_factor,
                    t.oblique_angle,
                    (h, v),
                    (flags & 2 != 0, flags & 4 != 0),
                    color,
                );
            }
            EntityType::AttributeEntity(a) => self.attribute(a, xf, color, inherit, depth),
            EntityType::Insert(ins) => self.insert(ins, xf, color, layer, depth),
            EntityType::Dimension(d) => {
                let base = d.base();
                let inner = Inherit { color, layer };
                if !self.block(&base.block_name, &Xf::ID, xf, &inner, depth) {
                    for x in e.explode() {
                        self.entity(&x, xf, &inner, depth + 1);
                    }
                }
            }
            EntityType::Hatch(h) => self.hatch(h, e, xf, color, inherit, depth),
            EntityType::MText(_)
            | EntityType::Leader(_)
            | EntityType::MultiLeader(_)
            | EntityType::MLine(_)
            | EntityType::Table(_)
            | EntityType::Helix(_)
            | EntityType::Mesh(_)
            | EntityType::PolyfaceMesh(_)
            | EntityType::PolygonMesh(_)
            | EntityType::Polyline(_)
            | EntityType::Polyline3D(_)
            | EntityType::Region(_)
            | EntityType::Solid3D(_)
            | EntityType::Body(_)
            | EntityType::Surface(_)
                if depth < MAX_DEPTH =>
            {
                // What it is made of carries its layer and colour.
                for x in e.explode() {
                    self.entity(&x, xf, inherit, depth + 1);
                }
            }
            _ => {}
        }
    }

    fn attribute(
        &mut self,
        a: &acadrust::entities::AttributeEntity,
        xf: &Xf,
        color: u32,
        inherit: &Inherit,
        depth: u8,
    ) {
        if a.flags.invisible {
            return;
        }
        if let Some(m) = a.embedded_mtext.as_ref().filter(|_| a.is_multiline) {
            for x in EntityType::MText(m.clone()).explode() {
                self.entity(&x, xf, inherit, depth + 1);
            }
            return;
        }
        let o = xf.then(&Xf::ocs(&a.normal, a.insertion_point.z));
        let h = match a.horizontal_alignment {
            HorizontalAlignment::Left => 0,
            HorizontalAlignment::Right => 2,
            _ => 1,
        };
        let v = match a.vertical_alignment {
            VerticalAlignment::Baseline => 0,
            VerticalAlignment::Bottom => 1,
            VerticalAlignment::Middle => 2,
            VerticalAlignment::Top => 3,
        };
        let v = if a.horizontal_alignment == HorizontalAlignment::Middle {
            2
        } else {
            v
        };
        let at = if (h, v) == (0, 0) {
            a.insertion_point
        } else {
            a.alignment_point
        };
        let flags = a.text_generation_flags;
        self.label(
            &a.value,
            (at.x, at.y),
            &o,
            a.height,
            a.rotation,
            a.width_factor,
            a.oblique_angle,
            (h, v),
            (flags & 2 != 0, flags & 4 != 0),
            color,
        );
    }

    /// The entities of block `name` through `xf` after `local`; false when there is no such
    /// block.
    fn block(&mut self, name: &str, local: &Xf, xf: &Xf, inherit: &Inherit, depth: u8) -> bool {
        let doc = self.doc;
        let Some(record) = doc.block_records.get(name) else {
            return false;
        };
        if depth >= MAX_DEPTH {
            return true;
        }
        let base = match doc.get_entity(record.block_entity_handle) {
            Some(EntityType::Block(b)) => (b.base_point.x, b.base_point.y),
            _ => (0.0, 0.0),
        };
        let m = xf.then(local).then(&Xf::translate(-base.0, -base.1));
        for h in &record.entity_handles {
            if self.full {
                break;
            }
            if let Some(e) = doc.get_entity(*h) {
                self.entity(e, &m, inherit, depth + 1);
            }
        }
        true
    }

    fn insert(
        &mut self,
        ins: &acadrust::entities::Insert,
        xf: &Xf,
        color: u32,
        layer: &str,
        depth: u8,
    ) {
        let p = &ins.insert_point;
        let o = xf.then(&Xf::ocs(&ins.normal, p.z));
        let inner = Inherit { color, layer };
        let (cols, rows) = (ins.column_count.max(1), ins.row_count.max(1));
        let place = Xf::translate(p.x, p.y).then(&Xf::rotate(ins.rotation));
        let scale = Xf::scale(ins.x_scale(), ins.y_scale());
        for r in 0..rows {
            for c in 0..cols {
                if self.full {
                    return;
                }
                let cell = Xf::translate(
                    f64::from(c) * ins.column_spacing,
                    f64::from(r) * ins.row_spacing,
                );
                let local = place.then(&cell).then(&scale);
                self.block(&ins.block_name, &local, &o, &inner, depth);
            }
        }
        // Attribute values sit where they are, not in the block.
        let parent = Inherit {
            color: FOREGROUND,
            layer,
        };
        for a in &ins.attributes {
            let common = &a.common;
            let look = self.layers.get(&common.layer.to_lowercase());
            if common.invisible || look.is_some_and(|l| !l.visible) {
                continue;
            }
            let c = match &common.color {
                Color::ByLayer => look.map_or(FOREGROUND, |l| l.color),
                Color::ByBlock => color,
                c => rgb_of(c).unwrap_or(FOREGROUND),
            };
            self.attribute(a, xf, c, &parent, depth);
        }
    }

    fn hatch(
        &mut self,
        h: &Hatch,
        e: &EntityType,
        xf: &Xf,
        color: u32,
        inherit: &Inherit,
        depth: u8,
    ) {
        let o = xf.then(&Xf::ocs(&h.normal, h.elevation));
        let rings: Vec<Vec<(f64, f64)>> = h.paths.iter().map(loop_points).collect();
        if h.is_solid || h.gradient_color.enabled {
            let c = if h.gradient_color.enabled {
                h.gradient_color
                    .colors
                    .first()
                    .and_then(|g| rgb_of(&g.color))
                    .unwrap_or(color)
            } else {
                color
            };
            self.fill(&o, c, 255, &rings);
            return;
        }
        // A pattern: its lines, unless there would be too many to draw.
        let (mut lo, mut hi) = (
            (f64::INFINITY, f64::INFINITY),
            (f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for p in rings.iter().flatten() {
            lo = (lo.0.min(p.0), lo.1.min(p.1));
            hi = (hi.0.max(p.0), hi.1.max(p.1));
        }
        let diagonal = ((hi.0 - lo.0).powi(2) + (hi.1 - lo.1).powi(2)).sqrt();
        let scale = if h.pattern_scale.abs() > 1e-12 {
            h.pattern_scale.abs()
        } else {
            1.0
        };
        let lines: f64 = h
            .pattern
            .lines
            .iter()
            .map(|l| {
                let spacing = (l.offset.x.powi(2) + l.offset.y.powi(2)).sqrt() * scale;
                let dashes = l.dash_lengths.len().max(1) as f64;
                if spacing > 1e-12 {
                    diagonal / spacing * dashes
                } else {
                    1.0
                }
            })
            .sum::<f64>()
            * if h.is_double { 2.0 } else { 1.0 };
        if !diagonal.is_finite() || lines > MAX_HATCH_LINES {
            self.fill(&o, color, 64, &rings);
            return;
        }
        for x in e.explode() {
            self.entity(&x, xf, inherit, depth + 1);
        }
    }

    fn finish(self, mut d: Drawing) -> Drawing {
        let mut floats =
            Vec::with_capacity(self.points * 2 + self.strokes.len() + self.fills.len());
        let mut strokes: Vec<_> = self.strokes.into_iter().collect();
        strokes.sort_by_key(|(c, _)| *c);
        for (color, buf) in strokes {
            let start = floats.len() as u32;
            floats.extend(buf);
            d.strokes.push(Run {
                color,
                alpha: 255,
                start,
                end: floats.len() as u32,
            });
        }
        let mut fills: Vec<_> = self.fills.into_iter().collect();
        // Light fills under solid ones.
        fills.sort_by_key(|((c, a), _)| (*a, *c));
        for ((color, alpha), buf) in fills {
            let start = floats.len() as u32;
            floats.extend(buf);
            d.fills.push(Run {
                color,
                alpha,
                start,
                end: floats.len() as u32,
            });
        }
        if let Some(o) = self.origin {
            let b =
                extents(&self.boxes).unwrap_or([self.min.0, self.min.1, self.max.0, self.max.1]);
            d.origin = [o.0, o.1];
            d.bounds = Some([
                (b[0] - o.0) as f32,
                (b[1] - o.1) as f32,
                (b[2] - o.0) as f32,
                (b[3] - o.1) as f32,
            ]);
        }
        d.colors = self.colors;
        d.labels = self.labels;
        d.truncated = self.full;
        d.floats = floats;
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn bulges_and_arcs() {
        // A half circle from (0,0) to (2,0), counter-clockwise: through (1,-1).
        let mut pts = Vec::new();
        bulge_points(&mut pts, (0.0, 0.0), (2.0, 0.0), 1.0);
        let mid = pts[pts.len() / 2];
        assert!(
            (mid.0 - 1.0).abs() < 1e-6 && (mid.1 + 1.0).abs() < 1e-6,
            "{mid:?}"
        );
        assert!(
            pts.iter()
                .all(|p| ((p.0 - 1.0).powi(2) + p.1.powi(2) - 1.0).abs() < 1e-9)
        );
        assert!((ccw_sweep(0.0, PI / 2.0) - PI / 2.0).abs() < 1e-12);
        assert!((ccw_sweep(PI / 2.0, 0.0) - 1.5 * PI).abs() < 1e-12);
        assert!((ccw_sweep(1.0, 1.0) - TAU).abs() < 1e-12);
    }

    #[test]
    fn coordinate_systems() {
        // Seen from below: x is mirrored.
        let o = Xf::ocs(&Vector3::new(0.0, 0.0, -1.0), 0.0);
        assert_eq!(o.at(1.0, 2.0), (-1.0, 2.0));
        let o = Xf::ocs(&Vector3::new(0.0, 0.0, 1.0), 5.0);
        assert_eq!(o.at(1.0, 2.0), (1.0, 2.0));
        // Rotate, then move.
        let m = Xf::translate(10.0, 0.0).then(&Xf::rotate(PI / 2.0));
        let (x, y) = m.at(1.0, 0.0);
        assert!((x - 10.0).abs() < 1e-12 && (y - 1.0).abs() < 1e-12);
    }

    #[test]
    fn nurbs_curves() {
        // A quadratic Bézier as a NURBS: through its middle at t = ½.
        let pts = nurbs(
            2,
            &[(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)],
            &[],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        assert_eq!(pts.first(), Some(&(0.0, 0.0)));
        assert_eq!(pts.last(), Some(&(2.0, 0.0)));
        let mid = pts[pts.len() / 2];
        assert!((mid.0 - 1.0).abs() < 1e-9 && (mid.1 - 1.0).abs() < 1e-9);
        // Knots that do not fit.
        assert!(nurbs(2, &[(0.0, 0.0), (1.0, 1.0)], &[], &[0.0, 1.0]).is_none());
    }

    #[test]
    fn areas_with_holes() {
        let square = |x: f64, y: f64, s: f64| vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]];
        let covered = |t: &[[f64; 2]]| -> f64 { t.chunks(3).map(area).sum() };
        // A 10 × 10 square with a 4 × 4 hole, and an island of 2 × 2 in the hole.
        let rings = vec![
            square(0.0, 0.0, 10.0),
            square(3.0, 3.0, 4.0),
            square(4.0, 4.0, 2.0),
        ];
        let t = triangulate(&rings);
        assert_eq!(t.len() % 3, 0);
        assert!(
            (covered(&t) - (100.0 - 16.0 + 4.0)).abs() < 1e-9,
            "{}",
            covered(&t)
        );
        // Two areas side by side.
        let t = triangulate(&[square(0.0, 0.0, 1.0), square(5.0, 0.0, 1.0)]);
        assert!((covered(&t) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn extents_leave_strays_out() {
        // A grid of small boxes, and one far away, very large: left out.
        let mut boxes: Vec<[f64; 4]> = (0..100)
            .map(|i| {
                let (x, y) = (f64::from(i % 10) * 10.0, f64::from(i / 10) * 10.0);
                [x, y, x + 5.0, y + 5.0]
            })
            .collect();
        boxes.push([-3e6, -2e6, 7e5, 9e5]);
        assert_eq!(extents(&boxes), Some([0.0, 0.0, 95.0, 95.0]));
        // A frame around everything stays.
        boxes.push([-10.0, -10.0, 110.0, 110.0]);
        assert_eq!(extents(&boxes), Some([-10.0, -10.0, 110.0, 110.0]));
        // Too few to tell.
        assert_eq!(extents(&boxes[..5]), None);
    }

    #[test]
    fn text_codes() {
        assert_eq!(special_codes("%%c20"), "⌀20");
        assert_eq!(special_codes("45%%d %%p0.1 100%%%"), "45° ±0.1 100%");
        assert_eq!(special_codes("%%uA%%u"), "A");
        assert_eq!(special_codes("%%177x"), "±x");
        assert_eq!(special_codes("plain"), "plain");
        assert_eq!(special_codes("end%%"), "end%%");
    }

    #[test]
    fn encoded_layout() {
        let d = Drawing {
            format: "DXF",
            version: None,
            units: None,
            objects: 0,
            layers: 0,
            paper: false,
            origin: [0.0; 2],
            bounds: None,
            colors: vec![],
            strokes: vec![],
            fills: vec![],
            labels: vec![],
            truncated: false,
            floats: vec![1.0, 2.5],
        };
        let b = d.encode();
        let n = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
        let json: serde_json::Value = serde_json::from_slice(&b[4..4 + n]).unwrap();
        assert_eq!(json["format"], "DXF");
        let at = (4 + n).div_ceil(4) * 4;
        assert_eq!(b.len(), at + 8);
        assert_eq!(
            f32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()),
            2.5
        );
    }

    /// Reads the drawings listed in `OMNILOOK_CAD` (one path per line) and reports how each went.
    #[test]
    #[ignore]
    fn real_drawings() {
        let Ok(list) = std::env::var("OMNILOOK_CAD") else {
            return;
        };
        let list = std::fs::read_to_string(&list).unwrap_or(list);
        for path in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let t = std::time::Instant::now();
            let r = std::panic::catch_unwind(|| read(Path::new(path)));
            let ms = t.elapsed().as_millis();
            match r {
                Ok(Ok(d)) => println!(
                    "ok {ms:>6}ms {:>8} pts {:>6} labels {:>6} objs {} {:?} trunc={} {}",
                    d.floats.len() / 2,
                    d.labels.len(),
                    d.objects,
                    d.version.unwrap_or("?"),
                    d.units,
                    d.truncated,
                    path
                ),
                Ok(Err(e)) => println!("ERR {ms:>6}ms {path}: {e}"),
                Err(_) => println!("PANIC {ms:>6}ms {path}"),
            }
        }
    }
}
