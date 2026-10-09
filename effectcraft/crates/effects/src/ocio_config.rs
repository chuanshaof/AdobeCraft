//! Custom OpenColorIO configuration files (`.ocio`) for the OCIO effects, read from the
//! published configuration format (a YAML document), not from OpenColorIO's source.
//!
//! Supported subset:
//!
//! * `search_path` (a string with `:`-separated entries or a list), resolved against the
//!   config's folder, for `FileTransform` sources;
//! * `roles` (any role name can stand for its colour space);
//! * `colorspaces`: `name`, `aliases`, `isdata`, and the transforms to / from the reference
//!   space — `to_reference` / `from_reference` (OCIO v1) or `to_scene_reference` /
//!   `from_scene_reference` (v2); a space with only one direction is inverted for the other;
//! * `displays`: display → views (`!<View> {name, colorspace}`);
//! * transforms: `MatrixTransform` (`matrix` 4×4 row-major, `offset`), `FileTransform` (the LUT
//!   and CDL files [`super::ocio::parse_file`] reads; `interpolation`, `ccc_id`),
//!   `ExponentTransform` (`value`), `LogTransform` (`base`), `LogAffineTransform` (`base`,
//!   `logSideSlope`, `logSideOffset`, `linSideSlope`, `linSideOffset`), `CDLTransform` (`slope`,
//!   `offset`, `power`, `sat`), `RangeTransform` (`min_in_value`, `max_in_value`,
//!   `min_out_value`, `max_out_value`) and `GroupTransform` (`children`), each with
//!   `direction: inverse`. Other transforms are reported by [`Config::unsupported`] and pass
//!   colours through.
//!
//! The YAML reader covers what configs use: block mappings and sequences, flow `{}` / `[]`
//! collections (also spanning lines), `!<Tag>` tags, quoted scalars, `|` / `>` block scalars
//! and comments.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::ocio::{Cdl, FileXform, load_file};

// ---------------------------------------------------------------- YAML subset

/// A parsed YAML node.
#[derive(Clone, Debug, PartialEq)]
pub enum Yaml {
    Str(String),
    Seq(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
    /// A `!<Tag>` node.
    Tagged(String, Box<Yaml>),
    Null,
}

impl Yaml {
    pub fn get(&self, k: &str) -> Option<&Yaml> {
        match self {
            Yaml::Map(m) => m.iter().find(|(key, _)| key == k).map(|(_, v)| v),
            Yaml::Tagged(_, v) => v.get(k),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Yaml::Str(s) => Some(s),
            Yaml::Tagged(_, v) => v.str(),
            _ => None,
        }
    }
    pub fn seq(&self) -> &[Yaml] {
        match self {
            Yaml::Seq(v) => v,
            Yaml::Tagged(_, v) => v.seq(),
            _ => &[],
        }
    }
    pub fn tag(&self) -> Option<&str> {
        match self {
            Yaml::Tagged(t, _) => Some(t),
            _ => None,
        }
    }
    fn f(&self) -> Option<f64> {
        self.str()?.trim().parse().ok()
    }
    fn nums(&self) -> Vec<f64> {
        match self {
            Yaml::Seq(v) => v.iter().filter_map(Yaml::f).collect(),
            Yaml::Str(_) => self.f().into_iter().collect(),
            Yaml::Tagged(_, v) => v.nums(),
            _ => vec![],
        }
    }
}

/// Strip a trailing comment (a `#` outside quotes, at the start or after whitespace).
fn strip_comment(line: &str) -> &str {
    let (mut sq, mut dq) = (false, false);
    let b = line.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'\'' if !dq => sq = !sq,
            b'"' if !sq => dq = !dq,
            b'#' if !sq && !dq && (i == 0 || b[i - 1].is_ascii_whitespace()) => return &line[..i],
            _ => {}
        }
    }
    line
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\''))) {
        return t[1..t.len() - 1].replace("\\\"", "\"").replace("''", "'");
    }
    t.to_string()
}

/// Split `key: value` at the first `:` followed by a space or the end (outside quotes/brackets).
fn split_key(s: &str) -> Option<(String, &str)> {
    let b = s.as_bytes();
    let (mut sq, mut dq, mut depth) = (false, false, 0i32);
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'\'' if !dq => sq = !sq,
            b'"' if !sq => dq = !dq,
            b'{' | b'[' if !sq && !dq => depth += 1,
            b'}' | b']' if !sq && !dq => depth -= 1,
            b':' if !sq && !dq && depth == 0 && (i + 1 == b.len() || b[i + 1] == b' ' || b[i + 1] == b'\t') => {
                let k = s[..i].trim();
                if k.is_empty() || k.starts_with('!') || k.starts_with('{') || k.starts_with('[') {
                    return None;
                }
                return Some((unquote(k), s[i + 1..].trim()));
            }
            _ => {}
        }
    }
    None
}

/// Bracket balance of a flow fragment (outside quotes).
fn balance(s: &str) -> i32 {
    let (mut sq, mut dq, mut d) = (false, false, 0);
    for c in s.chars() {
        match c {
            '\'' if !dq => sq = !sq,
            '"' if !sq => dq = !dq,
            '{' | '[' if !sq && !dq => d += 1,
            '}' | ']' if !sq && !dq => d -= 1,
            _ => {}
        }
    }
    d
}

struct Flow<'a> {
    s: &'a [u8],
    i: usize,
}

impl Flow<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn value(&mut self) -> Yaml {
        self.ws();
        match self.peek() {
            Some(b'{') => {
                self.i += 1;
                let mut m = vec![];
                loop {
                    self.ws();
                    match self.peek() {
                        None => break,
                        Some(b'}') => {
                            self.i += 1;
                            break;
                        }
                        Some(b',') => {
                            self.i += 1;
                            continue;
                        }
                        _ => {}
                    }
                    let k = self.scalar(true);
                    self.ws();
                    if self.peek() == Some(b':') {
                        self.i += 1;
                    }
                    let v = self.value();
                    m.push((unquote(&k), v));
                }
                Yaml::Map(m)
            }
            Some(b'[') => {
                self.i += 1;
                let mut v = vec![];
                loop {
                    self.ws();
                    match self.peek() {
                        None => break,
                        Some(b']') => {
                            self.i += 1;
                            break;
                        }
                        Some(b',') => {
                            self.i += 1;
                            continue;
                        }
                        _ => v.push(self.value()),
                    }
                }
                Yaml::Seq(v)
            }
            Some(b'!') => {
                let start = self.i;
                while self.i < self.s.len() && !(self.s[self.i] as char).is_whitespace() && self.s[self.i] != b'{' && self.s[self.i] != b'[' {
                    self.i += 1;
                }
                let tag = tag_name(std::str::from_utf8(&self.s[start..self.i]).unwrap_or(""));
                self.ws();
                let inner = match self.peek() {
                    Some(b'{') | Some(b'[') => self.value(),
                    _ => Yaml::Null,
                };
                Yaml::Tagged(tag, Box::new(inner))
            }
            _ => {
                let s = self.scalar(false);
                if s.is_empty() { Yaml::Null } else { Yaml::Str(unquote(&s)) }
            }
        }
    }
    /// A plain or quoted scalar, ending at `,` `}` `]` (and `:` + space for keys).
    fn scalar(&mut self, key: bool) -> String {
        self.ws();
        let start = self.i;
        if let Some(q @ (b'"' | b'\'')) = self.peek() {
            self.i += 1;
            while self.i < self.s.len() && self.s[self.i] != q {
                self.i += 1;
            }
            self.i = (self.i + 1).min(self.s.len());
            return String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
        }
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c == b',' || c == b'}' || c == b']' {
                break;
            }
            if key && c == b':' {
                break;
            }
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).trim().to_string()
    }
}

fn tag_name(t: &str) -> String {
    t.trim().trim_start_matches('!').trim_start_matches('<').trim_end_matches('>').to_string()
}

fn flow(s: &str) -> Yaml {
    Flow { s: s.as_bytes(), i: 0 }.value()
}

struct Lines {
    /// (indent, content without comment), blank lines removed.
    v: Vec<(usize, String)>,
}

impl Lines {
    fn new(text: &str) -> Lines {
        let v = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('%') && l.trim() != "---" && l.trim() != "...")
            .map(|l| {
                let c = strip_comment(l).trim_end();
                (c.len() - c.trim_start().len(), c.trim_start().to_string())
            })
            .filter(|(_, c)| !c.is_empty())
            .collect();
        Lines { v }
    }

    /// The value of a `key:` / `- ` remainder `rest` whose children are indented more than
    /// `indent` (from line `*i`).
    fn value(&self, rest: &str, indent: usize, i: &mut usize) -> Yaml {
        let rest = rest.trim();
        if rest.is_empty() {
            return match self.v.get(*i) {
                // A sequence may sit at the key's own indent.
                Some((ind, c)) if *ind > indent || (*ind == indent && (c == "-" || c.starts_with("- "))) => self.block(*ind, i),
                _ => Yaml::Null,
            };
        }
        if rest == "|" || rest == ">" || rest.starts_with("|-") || rest.starts_with(">-") || rest.starts_with("|+") || rest.starts_with(">+") {
            let mut out = vec![];
            while let Some((ind, c)) = self.v.get(*i) {
                if *ind <= indent {
                    break;
                }
                out.push(c.clone());
                *i += 1;
            }
            return Yaml::Str(out.join(if rest.starts_with('>') { " " } else { "\n" }));
        }
        if rest.starts_with('!') && !rest.contains('{') && !rest.contains('[') {
            let tag = tag_name(rest);
            let inner = match self.v.get(*i) {
                Some((ind, _)) if *ind > indent => self.block(*ind, i),
                _ => Yaml::Null,
            };
            return Yaml::Tagged(tag, Box::new(inner));
        }
        if rest.starts_with('{') || rest.starts_with('[') || (rest.starts_with('!') && (rest.contains('{') || rest.contains('['))) {
            // Flow collections may continue on the next lines.
            let mut text = rest.to_string();
            while balance(&text) > 0 && *i < self.v.len() {
                text.push(' ');
                text.push_str(&self.v[*i].1);
                *i += 1;
            }
            return flow(&text);
        }
        Yaml::Str(unquote(rest))
    }

    /// A block node whose lines start at `indent`.
    fn block(&self, indent: usize, i: &mut usize) -> Yaml {
        let Some((_, first)) = self.v.get(*i) else { return Yaml::Null };
        if first == "-" || first.starts_with("- ") {
            let mut items = vec![];
            while let Some((ind, c)) = self.v.get(*i) {
                if *ind != indent || !(c == "-" || c.starts_with("- ")) {
                    break;
                }
                *i += 1;
                let rest = c[1..].trim_start().to_string();
                // `- key: value` opens a mapping whose keys are indented to the key.
                if split_key(&rest).is_some() && !rest.starts_with('{') && !rest.starts_with('!') {
                    let inner = indent + 1 + (c.len() - 1 - c[1..].trim_start().len());
                    items.push(self.map_from(Some(rest), inner.max(indent + 2), i));
                } else {
                    items.push(self.value(&rest, indent, i));
                }
            }
            return Yaml::Seq(items);
        }
        if split_key(first).is_some() {
            return self.map_from(None, indent, i);
        }
        *i += 1;
        Yaml::Str(unquote(first))
    }

    /// A block mapping at `indent`; `first` is an entry already taken from a `- ` line.
    fn map_from(&self, first: Option<String>, indent: usize, i: &mut usize) -> Yaml {
        let mut m = vec![];
        if let Some(f) = first
            && let Some((k, rest)) = split_key(&f)
        {
            let v = self.value(rest, indent, i);
            m.push((k, v));
        }
        while let Some((ind, c)) = self.v.get(*i) {
            if *ind != indent {
                if *ind < indent {
                    break;
                }
                // Over-indented stray line: skip.
                *i += 1;
                continue;
            }
            let Some((k, rest)) = split_key(c) else { break };
            *i += 1;
            let v = self.value(rest, indent, i);
            m.push((k, v));
        }
        Yaml::Map(m)
    }
}

/// Parse a YAML document (the subset described in the module docs).
pub fn parse_yaml(text: &str) -> Yaml {
    let l = Lines::new(text);
    let mut i = 0;
    let indent = l.v.first().map_or(0, |(ind, _)| *ind);
    l.block(indent, &mut i)
}

// ---------------------------------------------------------------- transforms

/// A colour transform of a config.
#[derive(Clone, Debug)]
pub enum Xf {
    /// 4×4 row-major matrix (RGB of the first three rows/columns) plus offset.
    Matrix {
        m: [f64; 16],
        offset: [f64; 4],
    },
    File {
        src: String,
        xf: Option<Arc<FileXform>>,
        interp: u32,
        ccc: String,
    },
    Exponent([f64; 3]),
    Log {
        base: f64,
    },
    LogAffine {
        base: f64,
        log_slope: [f64; 3],
        log_offset: [f64; 3],
        lin_slope: [f64; 3],
        lin_offset: [f64; 3],
    },
    Cdl(Cdl),
    Range {
        min_in: f64,
        max_in: f64,
        min_out: f64,
        max_out: f64,
    },
    Group(Vec<Xf>),
    Inverse(Box<Xf>),
    /// An unsupported transform (by tag): identity.
    Unsupported(String),
}

fn vec3_of(y: Option<&Yaml>, d: f64) -> [f64; 3] {
    let v = y.map(Yaml::nums).unwrap_or_default();
    match v.len() {
        0 => [d; 3],
        1 | 2 => [v[0]; 3],
        _ => [v[0], v[1], v[2]],
    }
}

fn mat_apply(m: &[f64; 16], off: &[f64; 4], c: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| m[r * 4] * c[0] + m[r * 4 + 1] * c[1] + m[r * 4 + 2] * c[2] + off[r])
}

pub(crate) fn mat3_inverse(m: &[f64; 16]) -> Option<[[f64; 3]; 3]> {
    let a = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    (det.abs() > 1e-12).then(|| effectcraft_color::space::invert(&a))
}

fn log_base(v: f64, base: f64) -> f64 {
    v.max(f64::MIN_POSITIVE).ln() / base.ln()
}

impl Xf {
    fn from_yaml(y: &Yaml, dir: &Path, search: &[String]) -> Xf {
        let tag = y.tag().unwrap_or("");
        let body = match y {
            Yaml::Tagged(_, b) => b.as_ref(),
            other => other,
        };
        let inverse = body.get("direction").and_then(Yaml::str).is_some_and(|d| d.eq_ignore_ascii_case("inverse"));
        let xf = match tag {
            "MatrixTransform" => {
                let v = body.get("matrix").map(Yaml::nums).unwrap_or_default();
                let mut m = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
                if v.len() == 16 {
                    m.copy_from_slice(&v);
                } else if v.len() == 9 {
                    for r in 0..3 {
                        for c in 0..3 {
                            m[r * 4 + c] = v[r * 3 + c];
                        }
                    }
                }
                let o = body.get("offset").map(Yaml::nums).unwrap_or_default();
                let mut offset = [0.0; 4];
                for (k, v) in o.iter().take(4).enumerate() {
                    offset[k] = *v;
                }
                Xf::Matrix { m, offset }
            }
            "FileTransform" => {
                let src = body.get("src").and_then(Yaml::str).unwrap_or("").to_string();
                let interp = match body.get("interpolation").and_then(Yaml::str).unwrap_or("linear").to_ascii_lowercase().as_str() {
                    "nearest" => 0,
                    "tetrahedral" | "best" => 2,
                    _ => 1,
                };
                let ccc = body.get("cccid").or_else(|| body.get("ccc_id")).and_then(Yaml::str).unwrap_or("").to_string();
                let xf = resolve(&src, dir, search).and_then(|p| load_file(&p.to_string_lossy()));
                Xf::File { src, xf, interp, ccc }
            }
            "ExponentTransform" => Xf::Exponent(vec3_of(body.get("value"), 1.0)),
            "LogTransform" => Xf::Log { base: body.get("base").and_then(Yaml::f).unwrap_or(2.0) },
            "LogAffineTransform" => Xf::LogAffine {
                base: body.get("base").and_then(Yaml::f).unwrap_or(2.0),
                log_slope: vec3_of(body.get("logSideSlope"), 1.0),
                log_offset: vec3_of(body.get("logSideOffset"), 0.0),
                lin_slope: vec3_of(body.get("linSideSlope"), 1.0),
                lin_offset: vec3_of(body.get("linSideOffset"), 0.0),
            },
            "CDLTransform" => Xf::Cdl(Cdl {
                slope: vec3_of(body.get("slope"), 1.0),
                offset: vec3_of(body.get("offset"), 0.0),
                power: vec3_of(body.get("power"), 1.0),
                sat: body.get("sat").and_then(Yaml::f).unwrap_or(1.0),
            }),
            "RangeTransform" => Xf::Range {
                min_in: body.get("min_in_value").and_then(Yaml::f).unwrap_or(0.0),
                max_in: body.get("max_in_value").and_then(Yaml::f).unwrap_or(1.0),
                min_out: body.get("min_out_value").and_then(Yaml::f).unwrap_or(0.0),
                max_out: body.get("max_out_value").and_then(Yaml::f).unwrap_or(1.0),
            },
            "GroupTransform" => Xf::Group(body.get("children").map(|c| c.seq().iter().map(|t| Xf::from_yaml(t, dir, search)).collect()).unwrap_or_default()),
            other => Xf::Unsupported(other.to_string()),
        };
        if inverse { Xf::Inverse(Box::new(xf)) } else { xf }
    }

    /// Apply forward (or inverse) to linear-or-encoded RGB.
    pub fn apply(&self, c: [f64; 3], inverse: bool) -> [f64; 3] {
        match self {
            Xf::Inverse(x) => x.apply(c, !inverse),
            Xf::Matrix { m, offset } => {
                if !inverse {
                    return mat_apply(m, offset, c);
                }
                match mat3_inverse(m) {
                    Some(inv) => {
                        let d = [c[0] - offset[0], c[1] - offset[1], c[2] - offset[2]];
                        effectcraft_color::space::mul_vec(&inv, d)
                    }
                    None => c,
                }
            }
            Xf::File { xf, interp, ccc, .. } => match xf {
                Some(x) => x.apply(c.map(|v| v as f32), *interp, inverse, ccc).map(|v| v as f64),
                None => c,
            },
            Xf::Exponent(e) => [0, 1, 2].map(|k| {
                let p = if inverse { 1.0 / e[k].max(1e-9) } else { e[k] };
                c[k].max(0.0).powf(p)
            }),
            Xf::Log { base } => {
                if inverse {
                    c.map(|v| base.powf(v))
                } else {
                    c.map(|v| log_base(v, *base))
                }
            }
            Xf::LogAffine { base, log_slope, log_offset, lin_slope, lin_offset } => [0, 1, 2].map(|k| {
                if inverse {
                    (base.powf((c[k] - log_offset[k]) / log_slope[k]) - lin_offset[k]) / lin_slope[k]
                } else {
                    log_slope[k] * log_base(lin_slope[k] * c[k] + lin_offset[k], *base) + log_offset[k]
                }
            }),
            Xf::Cdl(cdl) => {
                if inverse {
                    cdl.invert(c, true)
                } else {
                    cdl.apply(c, true)
                }
            }
            Xf::Range { min_in, max_in, min_out, max_out } => {
                let (a0, a1, b0, b1) = if inverse { (*min_out, *max_out, *min_in, *max_in) } else { (*min_in, *max_in, *min_out, *max_out) };
                let k = if (a1 - a0).abs() > 1e-12 { (b1 - b0) / (a1 - a0) } else { 0.0 };
                c.map(|v| b0 + (v - a0) * k)
            }
            Xf::Group(list) => {
                let mut v = c;
                if inverse {
                    for x in list.iter().rev() {
                        v = x.apply(v, true);
                    }
                } else {
                    for x in list {
                        v = x.apply(v, false);
                    }
                }
                v
            }
            Xf::Unsupported(_) => c,
        }
    }

    fn unsupported(&self, out: &mut Vec<String>) {
        match self {
            Xf::Unsupported(t) => out.push(t.clone()),
            Xf::Inverse(x) => x.unsupported(out),
            Xf::Group(v) => v.iter().for_each(|x| x.unsupported(out)),
            Xf::File { src, xf: None, .. } => out.push(format!("FileTransform (missing `{src}`)")),
            _ => {}
        }
    }
}

/// Find a FileTransform source on the search path (relative to the config's folder).
fn resolve(src: &str, dir: &Path, search: &[String]) -> Option<PathBuf> {
    let p = Path::new(src);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    let mut dirs: Vec<PathBuf> = search.iter().map(|s| dir.join(s)).collect();
    dirs.push(dir.to_path_buf());
    dirs.into_iter().map(|d| d.join(src)).find(|c| c.exists())
}

// ---------------------------------------------------------------- the config

/// A colour space of a config.
#[derive(Clone, Debug)]
pub struct ConfigSpace {
    pub name: String,
    pub aliases: Vec<String>,
    pub family: String,
    pub is_data: bool,
    pub to_ref: Option<Xf>,
    pub from_ref: Option<Xf>,
}

impl ConfigSpace {
    fn to_reference(&self, c: [f64; 3]) -> [f64; 3] {
        match (&self.to_ref, &self.from_ref) {
            (Some(x), _) => x.apply(c, false),
            (None, Some(x)) => x.apply(c, true),
            _ => c,
        }
    }
    fn reference_to(&self, c: [f64; 3]) -> [f64; 3] {
        match (&self.from_ref, &self.to_ref) {
            (Some(x), _) => x.apply(c, false),
            (None, Some(x)) => x.apply(c, true),
            _ => c,
        }
    }
}

/// A parsed `.ocio` configuration.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub name: String,
    pub search_path: Vec<String>,
    pub roles: Vec<(String, String)>,
    pub spaces: Vec<ConfigSpace>,
    /// Display → [(view, colour space)].
    pub displays: Vec<(String, Vec<(String, String)>)>,
}

impl Config {
    /// Parse config text; FileTransform sources resolve against `dir`.
    pub fn parse(text: &str, dir: &Path) -> Result<Config, String> {
        let y = parse_yaml(text);
        if !matches!(y, Yaml::Map(_)) {
            return Err("not an OCIO config (expected a YAML mapping)".into());
        }
        let search_path: Vec<String> = match y.get("search_path") {
            Some(Yaml::Seq(v)) => v.iter().filter_map(|s| s.str().map(str::to_string)).collect(),
            Some(s) => s.str().map(|s| s.split(':').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()).unwrap_or_default(),
            None => vec![],
        };
        let roles = match y.get("roles") {
            Some(Yaml::Map(m)) => m.iter().filter_map(|(k, v)| v.str().map(|s| (k.clone(), s.to_string()))).collect(),
            _ => vec![],
        };
        let mut spaces = vec![];
        for list in ["colorspaces", "display_colorspaces"] {
            for cs in y.get(list).map(Yaml::seq).unwrap_or(&[]) {
                let Some(name) = cs.get("name").and_then(Yaml::str) else { continue };
                let xf = |keys: &[&str]| keys.iter().find_map(|k| cs.get(k)).filter(|v| !matches!(v, Yaml::Null)).map(|v| Xf::from_yaml(v, dir, &search_path));
                spaces.push(ConfigSpace {
                    name: name.to_string(),
                    aliases: cs.get("aliases").map(|a| a.seq().iter().filter_map(|s| s.str().map(str::to_string)).collect()).unwrap_or_default(),
                    family: cs.get("family").and_then(Yaml::str).unwrap_or("").to_string(),
                    is_data: cs.get("isdata").and_then(Yaml::str).is_some_and(|v| v == "true"),
                    to_ref: xf(&["to_reference", "to_scene_reference", "to_display_reference"]),
                    from_ref: xf(&["from_reference", "from_scene_reference", "from_display_reference"]),
                });
            }
        }
        if spaces.is_empty() {
            return Err("the config has no colorspaces".into());
        }
        let displays = match y.get("displays") {
            Some(Yaml::Map(m)) => m
                .iter()
                .map(|(d, views)| {
                    let v = views.seq().iter().filter_map(|v| Some((v.get("name")?.str()?.to_string(), v.get("colorspace")?.str()?.to_string()))).collect();
                    (d.clone(), v)
                })
                .collect(),
            _ => vec![],
        };
        Ok(Config { name: y.get("name").and_then(Yaml::str).unwrap_or("").to_string(), search_path, roles, spaces, displays })
    }

    pub fn space_names(&self) -> Vec<&str> {
        self.spaces.iter().map(|s| s.name.as_str()).collect()
    }

    /// A colour space by name, alias or role (case-insensitive).
    pub fn space(&self, name: &str) -> Option<&ConfigSpace> {
        let n = name.trim();
        let by = |n: &str| self.spaces.iter().find(|s| s.name.eq_ignore_ascii_case(n) || s.aliases.iter().any(|a| a.eq_ignore_ascii_case(n)));
        by(n).or_else(|| self.roles.iter().find(|(r, _)| r.eq_ignore_ascii_case(n)).and_then(|(_, s)| by(s)))
    }

    /// The colour space of a display's view.
    pub fn view_space(&self, display: &str, view: &str) -> Option<&ConfigSpace> {
        let (_, views) = self.displays.iter().find(|(d, _)| d.eq_ignore_ascii_case(display.trim())).or(self.displays.first())?;
        let (_, cs) = views.iter().find(|(v, _)| v.eq_ignore_ascii_case(view.trim())).or(views.first())?;
        self.space(cs)
    }

    /// Convert one colour from space `a` to space `b` (through the reference space; data
    /// spaces pass through).
    pub fn convert(&self, a: &ConfigSpace, b: &ConfigSpace, c: [f64; 3]) -> [f64; 3] {
        if a.is_data || b.is_data || a.name == b.name {
            return c;
        }
        b.reference_to(a.to_reference(c))
    }

    /// Transforms this module can't apply (by tag), for warnings.
    pub fn unsupported(&self) -> Vec<String> {
        let mut out = vec![];
        for s in &self.spaces {
            for x in [&s.to_ref, &s.from_ref].into_iter().flatten() {
                x.unsupported(&mut out);
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

type ConfigCache = Mutex<HashMap<String, Option<Arc<Config>>>>;

/// Load (and cache) a config from a `.ocio` path or the config text itself.
pub fn load_config(src: &str) -> Option<Arc<Config>> {
    let t = src.trim();
    if t.is_empty() {
        return None;
    }
    static CACHE: OnceLock<ConfigCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(m) = cache.lock()
        && let Some(v) = m.get(src)
    {
        return v.clone();
    }
    let parsed = if src.contains('\n') {
        Config::parse(src, Path::new("."))
    } else {
        let p = Path::new(t);
        std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|text| Config::parse(&text, p.parent().unwrap_or(Path::new("."))))
    }
    .ok()
    .map(Arc::new);
    if let Ok(mut m) = cache.lock() {
        m.insert(src.to_string(), parsed.clone());
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
ocio_profile_version: 2

name: Test Config   # a comment
search_path: "luts:other"
roles:
  scene_linear: lin
  color_timing: "log2"

displays:
  sRGB:
    - !<View> {name: Standard, colorspace: gamma22}
    - !<View> {name: Raw, colorspace: raw}

colorspaces:
  - !<ColorSpace>
    name: lin
    family: ""
    description: |
      The reference.
      Linear.
    isdata: false

  - !<ColorSpace>
    name: gamma22
    aliases: [g22, "gamma 2.2"]
    to_scene_reference: !<ExponentTransform> {value: [2.2, 2.2, 2.2, 1]}

  - !<ColorSpace>
    name: half
    from_reference: !<MatrixTransform> {matrix: [0.5, 0, 0, 0,
                                                0, 0.5, 0, 0,
                                                0, 0, 0.5, 0,
                                                0, 0, 0, 1], offset: [0.1, 0.1, 0.1, 0]}

  - !<ColorSpace>
    name: log2
    from_reference: !<GroupTransform>
      children:
        - !<MatrixTransform> {matrix: [2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1]}
        - !<LogTransform> {base: 2}

  - !<ColorSpace>
    name: affine
    to_reference: !<LogAffineTransform> {base: 10, logSideSlope: 0.5, logSideOffset: 0.2, linSideSlope: 2, linSideOffset: 0.01, direction: inverse}

  - !<ColorSpace>
    name: lut
    to_reference: !<FileTransform> {src: double.cube, interpolation: linear}

  - !<ColorSpace>
    name: raw
    isdata: true

  - !<ColorSpace>
    name: weird
    to_reference: !<ExposureContrastTransform> {exposure: 1}
"#;

    fn close(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < tol)
    }

    #[test]
    fn yaml_subset() {
        let y = parse_yaml("a: 1\nb:\n  - x\n  - {k: [1, 2], s: 'q: r'}\nc: !<T> {v: 3}\nd:\n- 4\n- 5\n");
        assert_eq!(y.get("a").and_then(Yaml::str), Some("1"));
        assert_eq!(y.get("b").unwrap().seq().len(), 2);
        assert_eq!(y.get("b").unwrap().seq()[1].get("k").unwrap().nums(), vec![1.0, 2.0]);
        assert_eq!(y.get("b").unwrap().seq()[1].get("s").and_then(Yaml::str), Some("q: r"));
        assert_eq!(y.get("c").unwrap().tag(), Some("T"));
        assert_eq!(y.get("c").unwrap().get("v").and_then(Yaml::str), Some("3"));
        assert_eq!(y.get("d").unwrap().seq().len(), 2, "a sequence at the key's indent");
    }

    #[test]
    fn parses_spaces_roles_displays_and_converts() {
        let dir = std::env::temp_dir().join(format!("ec-ocio-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("luts")).unwrap();
        // A 1D LUT that doubles (0..1 → 0..2).
        std::fs::write(dir.join("luts/double.cube"), "LUT_1D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n0 0 0\n2 2 2\n").unwrap();
        let c = Config::parse(CONFIG, &dir).unwrap();
        assert_eq!(c.name, "Test Config");
        assert_eq!(c.search_path, vec!["luts", "other"]);
        assert_eq!(c.space_names(), vec!["lin", "gamma22", "half", "log2", "affine", "lut", "raw", "weird"]);
        // Roles and aliases resolve.
        assert_eq!(c.space("scene_linear").unwrap().name, "lin");
        assert_eq!(c.space("G22").unwrap().name, "gamma22");
        assert_eq!(c.space("gamma 2.2").unwrap().name, "gamma22");
        assert_eq!(c.view_space("sRGB", "Standard").unwrap().name, "gamma22");
        let sp = |n: &str| c.space(n).unwrap();
        // gamma22 → lin: decode with the exponent.
        let g = c.convert(sp("gamma22"), sp("lin"), [0.5, 0.25, 1.0]);
        assert!(close(g, [0.5f64.powf(2.2), 0.25f64.powf(2.2), 1.0], 1e-9), "{g:?}");
        // lin → gamma22: the inverse (only to_reference given).
        let back = c.convert(sp("lin"), sp("gamma22"), g);
        assert!(close(back, [0.5, 0.25, 1.0], 1e-9), "{back:?}");
        // lin → half: matrix and offset; and back through the inverse matrix.
        let h = c.convert(sp("lin"), sp("half"), [0.4, 0.6, 1.0]);
        assert!(close(h, [0.3, 0.4, 0.6], 1e-9), "{h:?}");
        assert!(close(c.convert(sp("half"), sp("lin"), h), [0.4, 0.6, 1.0], 1e-9));
        // lin → log2: a group (×2 then log2): 0.5 → 0; 2 → 2.
        let l = c.convert(sp("lin"), sp("log2"), [0.5, 2.0, 4.0]);
        assert!(close(l, [0.0, 2.0, 3.0], 1e-9), "{l:?}");
        assert!(close(c.convert(sp("log2"), sp("lin"), l), [0.5, 2.0, 4.0], 1e-9));
        // LogAffine (inverse direction as to_reference) round-trips.
        let a = c.convert(sp("lin"), sp("affine"), [0.18, 0.5, 1.0]);
        let expect = [0.18f64, 0.5, 1.0].map(|v| 0.5 * (2.0 * v + 0.01).log10() + 0.2);
        assert!(close(a, expect, 1e-9), "{a:?} vs {expect:?}");
        assert!(close(c.convert(sp("affine"), sp("lin"), a), [0.18, 0.5, 1.0], 1e-9));
        // FileTransform found on the search path.
        let f = c.convert(sp("lut"), sp("lin"), [0.25, 0.5, 0.0]);
        assert!(close(f, [0.5, 1.0, 0.0], 1e-6), "{f:?}");
        // Data spaces pass through; unsupported transforms are reported.
        assert_eq!(c.convert(sp("raw"), sp("gamma22"), [0.3, 0.3, 0.3]), [0.3, 0.3, 0.3]);
        assert_eq!(c.unsupported(), vec!["ExposureContrastTransform".to_string()]);
        // Loading from a file path (cached), and errors.
        std::fs::write(dir.join("config.ocio"), CONFIG).unwrap();
        let loaded = load_config(&dir.join("config.ocio").to_string_lossy()).unwrap();
        assert_eq!(loaded.spaces.len(), 8);
        assert!(load_config("/no/such/config.ocio").is_none());
        assert!(Config::parse("just: text\n", &dir).is_err());
    }
    /// The OCIO Color Space and Display Transform effects with Configuration ▸ Custom.
    #[test]
    fn effects_use_a_custom_config() {
        use effectcraft_keyframe::Value;
        let cfg = "ocio_profile_version: 1\nroles:\n  default: lin\ndisplays:\n  Monitor:\n    - !<View> {name: Video, colorspace: g2}\ncolorspaces:\n  - !<ColorSpace>\n    name: lin\n  - !<ColorSpace>\n    name: g2\n    to_reference: !<ExponentTransform> {value: [2, 2, 2, 1]}\n";
        let img = crate::Image::filled(2, 2, [0.5, 0.25, 1.0, 1.0]);
        let fx = |id: &str, extra: &[(&str, Value)]| {
            let mut v = vec![("config", Value::Enum(1)), ("configFile", Value::Str(cfg.into()))];
            v.extend(extra.iter().cloned());
            crate::run_fx(id, &v, img.clone(), 0.0, crate::EffectEnv::default()).img.data[0]
        };
        let o = fx("ec.color.ociocolorspace", &[("sourceName", Value::Str("g2".into())), ("destinationName", Value::Str("default".into()))]);
        assert!((o[0] - 0.25).abs() < 1e-6 && (o[1] - 0.0625).abs() < 1e-6 && (o[2] - 1.0).abs() < 1e-6, "{o:?}");
        // Inverse direction.
        let o = fx(
            "ec.color.ociocolorspace",
            &[("sourceName", Value::Str("g2".into())), ("destinationName", Value::Str("lin".into())), ("direction", Value::Enum(1))],
        );
        assert!((o[0] - 0.5f32.sqrt()).abs() < 1e-6, "{o:?}");
        // Display ▸ view → its colour space.
        let o = fx(
            "ec.color.ociodisplay",
            &[("sourceName", Value::Str("lin".into())), ("displayName", Value::Str("Monitor".into())), ("viewName", Value::Str("Video".into()))],
        );
        assert!((o[0] - 0.5f32.sqrt()).abs() < 1e-6, "{o:?}");
        // An unreadable config leaves pixels alone.
        let o = crate::run_fx(
            "ec.color.ociocolorspace",
            &[("config", Value::Enum(1)), ("configFile", Value::Str("/missing.ocio".into()))],
            img.clone(),
            0.0,
            crate::EffectEnv::default(),
        );
        assert_eq!(o.img.data, img.data);
    }
}
