//! Minimaler MATLAB-v5-MAT-Parser (little endian) – genug, um `.fig`-Dateien zu lesen.

use anyhow::{bail, Result};
use flate2::read::ZlibDecoder;
use std::io::Read;

#[derive(Debug, Clone)]
pub enum Value {
    Num(Vec<f64>),
    Text(String),
    Struct { names: Vec<String>, elems: Vec<Vec<Value>> },
    Cell(Vec<Value>),
    Other,
}

impl Value {
    pub fn field(&self, elem: usize, name: &str) -> Option<&Value> {
        if let Value::Struct { names, elems } = self {
            let i = names.iter().position(|n| n == name)?;
            elems.get(elem)?.get(i)
        } else {
            None
        }
    }
    pub fn text(&self) -> Option<&str> {
        if let Value::Text(s) = self { Some(s) } else { None }
    }
    pub fn nums(&self) -> Option<&[f64]> {
        if let Value::Num(v) = self { Some(v) } else { None }
    }
    pub fn len(&self) -> usize {
        match self {
            Value::Struct { elems, .. } => elems.len(),
            _ => 1,
        }
    }
}

struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
}

/// Maximale Verschachtelungstiefe und entpackte Größe, damit kaputte Dateien nicht den Stack
/// oder den Speicher sprengen.
const MAX_DEPTH: usize = 64;
const MAX_INFLATED: u64 = 1 << 30;

fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn u32le_checked(b: &[u8]) -> Result<u32> {
    match b.get(..4) {
        Some(b) => Ok(u32le(b)),
        None => bail!("Element zu kurz"),
    }
}

impl<'a> Reader<'a> {
    fn next(&mut self) -> Result<(u32, &'a [u8])> {
        if self.pos + 8 > self.d.len() {
            bail!("unerwartetes Dateiende");
        }
        let w = u32le(&self.d[self.pos..]);
        let (ty, n, hdr, small) = if w >> 16 != 0 {
            (w & 0xffff, (w >> 16) as usize, 4, true)
        } else {
            (w, u32le(&self.d[self.pos + 4..]) as usize, 8, false)
        };
        let start = self.pos + hdr;
        if start.checked_add(n).is_none_or(|end| end > self.d.len()) {
            bail!("Element ragt über Dateiende");
        }
        let data = &self.d[start..start + n];
        let total = if small {
            8
        } else if ty == 15 {
            hdr + n
        } else {
            hdr + n.div_ceil(8) * 8
        };
        self.pos = (self.pos + total).min(self.d.len());
        Ok((ty, data))
    }
}

fn to_f64(ty: u32, d: &[u8]) -> Result<Vec<f64>> {
    macro_rules! conv {
        ($t:ty, $n:expr) => {
            d.chunks_exact($n)
                .map(|c| <$t>::from_le_bytes(c.try_into().unwrap()) as f64)
                .collect()
        };
    }
    Ok(match ty {
        1 => conv!(i8, 1),
        2 => conv!(u8, 1),
        3 => conv!(i16, 2),
        4 => conv!(u16, 2),
        5 => conv!(i32, 4),
        6 => conv!(u32, 4),
        7 => conv!(f32, 4),
        9 => conv!(f64, 8),
        12 => conv!(i64, 8),
        13 => conv!(u64, 8),
        _ => bail!("unbekannter Zahlentyp {ty}"),
    })
}

fn parse_matrix(data: &[u8], depth: usize) -> Result<(String, Value)> {
    if depth > MAX_DEPTH {
        bail!("Verschachtelung zu tief");
    }
    if data.is_empty() {
        // leeres Feld ([]) ist als miMATRIX mit 0 Byte abgelegt
        return Ok((String::new(), Value::Num(Vec::new())));
    }
    let mut r = Reader { d: data, pos: 0 };
    let (_, flags) = r.next()?;
    let class = *flags.first().ok_or_else(|| anyhow::anyhow!("Flags fehlen"))?;
    let (_, dims) = r.next()?;
    let n: usize = dims
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap()).max(0) as usize)
        .try_fold(1usize, |a, d| a.checked_mul(d))
        .ok_or_else(|| anyhow::anyhow!("Dimensionen zu groß"))?;
    // Jedes Element braucht mindestens 8 Byte; größere n sind in dieser Datei unmöglich.
    let cap = n.min(data.len() / 8);
    let (_, name) = r.next()?;
    let name = String::from_utf8_lossy(name).into_owned();
    let v = match class {
        1 => {
            let mut items = Vec::with_capacity(cap);
            for _ in 0..n {
                let (_, d) = r.next()?;
                items.push(parse_matrix(d, depth + 1)?.1);
            }
            Value::Cell(items)
        }
        2 | 3 => {
            if class == 3 {
                r.next()?; // Klassenname
            }
            let (_, l) = r.next()?;
            let len = u32le_checked(l)? as usize;
            let (_, nm) = r.next()?;
            let names: Vec<String> = nm
                .chunks(len.max(1))
                .map(|c| String::from_utf8_lossy(c).trim_end_matches('\0').to_string())
                .collect();
            let mut elems = Vec::with_capacity(cap);
            for _ in 0..n {
                let mut row = Vec::with_capacity(names.len().min(data.len() / 8));
                for _ in 0..names.len() {
                    let (_, d) = r.next()?;
                    row.push(parse_matrix(d, depth + 1)?.1);
                }
                elems.push(row);
            }
            Value::Struct { names, elems }
        }
        4 => {
            if n == 0 || r.pos + 8 > r.d.len() {
                Value::Text(String::new())
            } else {
                let (t, d) = r.next()?;
                Value::Text(match t {
                    4 => {
                        let u: Vec<u16> = d
                            .chunks_exact(2)
                            .map(|c| u16::from_le_bytes([c[0], c[1]]))
                            .collect();
                        String::from_utf16_lossy(&u)
                    }
                    _ => String::from_utf8_lossy(d).into_owned(),
                })
            }
        }
        6..=15 => {
            if r.pos + 8 > r.d.len() {
                Value::Num(Vec::new())
            } else {
                let (t, d) = r.next()?;
                Value::Num(to_f64(t, d)?)
            }
        }
        _ => Value::Other,
    };
    Ok((name, v))
}

/// Liest die Variable `hgS_*` (das Figure-Objekt) aus einer `.fig`/`.mat`-Datei.
pub fn load_figure(path: &str) -> Result<Value> {
    parse_figure(&std::fs::read(path)?)
}

/// Wie [`load_figure`], aber auf bereits eingelesenen Bytes.
pub fn parse_figure(bytes: &[u8]) -> Result<Value> {
    if bytes.len() < 136 || !bytes.starts_with(b"MATLAB 5.0") {
        bail!("Keine MATLAB-v5-Datei (v7.3/HDF5 wird nicht unterstützt)");
    }
    if &bytes[126..128] != b"IM" {
        bail!("Big-Endian-Dateien werden nicht unterstützt");
    }
    let mut top = Reader { d: bytes, pos: 128 };
    let mut last_err = String::new();
    while let Ok((ty, data)) = top.next() {
        let parsed = match ty {
            14 => parse_matrix(data, 0),
            15 => {
                let mut buf = Vec::new();
                if let Err(e) = ZlibDecoder::new(data).take(MAX_INFLATED).read_to_end(&mut buf) {
                    last_err = e.to_string();
                    continue;
                }
                let mut inner = Reader { d: &buf, pos: 0 };
                match inner.next() {
                    Ok((14, d)) => parse_matrix(d, 0),
                    _ => continue,
                }
            }
            _ => continue,
        };
        match parsed {
            Ok((name, v)) if name.starts_with("hgS_") => return Ok(v),
            Ok(_) => {}
            Err(e) => last_err = e.to_string(),
        }
    }
    bail!("Keine Figure-Daten (hgS_*) gefunden ({last_err})")
}

pub struct RawLine {
    pub name: String,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

/// Sammelt rekursiv alle Linien-Objekte der Figure.
pub fn collect_lines(v: &Value, out: &mut Vec<RawLine>) {
    for i in 0..v.len() {
        let ty = v.field(i, "type").and_then(Value::text).unwrap_or("");
        if ty.contains("line") {
            if let Some(p) = v.field(i, "properties") {
                let x = p.field(0, "XData").and_then(Value::nums);
                let y = p.field(0, "YData").and_then(Value::nums);
                if let (Some(x), Some(y)) = (x, y) {
                    let name = p
                        .field(0, "DisplayName")
                        .and_then(Value::text)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .unwrap_or_else(|| format!("Signal {}", out.len() + 1));
                    out.push(RawLine { name, x: x.to_vec(), y: y.to_vec() });
                }
            }
        }
        if let Some(c) = v.field(i, "children") {
            collect_lines(c, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;

    fn pad8(v: &mut Vec<u8>) {
        while v.len() % 8 != 0 {
            v.push(0);
        }
    }

    fn elem(ty: u32, data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(ty.to_le_bytes());
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        pad8(&mut v);
        v
    }

    fn matrix(class: u8, dims: &[i32], name: &str, body: &[Vec<u8>]) -> Vec<u8> {
        let mut m = elem(6, &[class, 0, 0, 0, 0, 0, 0, 0]);
        let d: Vec<u8> = dims.iter().flat_map(|d| d.to_le_bytes()).collect();
        m.extend(elem(5, &d));
        m.extend(elem(1, name.as_bytes()));
        for b in body {
            m.extend(b);
        }
        elem(14, &m)
    }

    fn doubles(name: &str, v: &[f64]) -> Vec<u8> {
        let raw: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        matrix(6, &[1, v.len() as i32], name, &[elem(9, &raw)])
    }

    fn text(name: &str, s: &str) -> Vec<u8> {
        matrix(4, &[1, s.len() as i32], name, &[elem(1, s.as_bytes())])
    }

    fn strukt(name: &str, fields: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut nm = Vec::new();
        for (f, _) in fields {
            let mut b = f.as_bytes().to_vec();
            b.resize(32, 0);
            nm.extend(b);
        }
        let mut body = vec![elem(5, &32u32.to_le_bytes()), elem(1, &nm)];
        body.extend(fields.iter().map(|(_, v)| v.clone()));
        matrix(2, &[1, 1], name, &body)
    }

    fn header() -> Vec<u8> {
        let mut h = b"MATLAB 5.0 MAT-file".to_vec();
        h.resize(124, b' ');
        h.extend([0, 1, b'I', b'M']);
        h
    }

    fn sample_line(name: &str) -> Vec<u8> {
        let props = strukt(
            "",
            &[
                ("XData", doubles("", &[0.0, 1.0, 2.0])),
                ("YData", doubles("", &[5.0, 6.0, 7.0])),
                ("DisplayName", text("", name)),
            ],
        );
        strukt("", &[("type", text("", "line")), ("properties", props)])
    }

    fn sample_file(compressed: bool) -> Vec<u8> {
        let fig = strukt(
            "hgS_070000",
            &[("type", text("", "figure")), ("children", sample_line("sig"))],
        );
        let mut f = header();
        if compressed {
            let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
            e.write_all(&fig).unwrap();
            f.extend(elem(15, &e.finish().unwrap()));
        } else {
            f.extend(fig);
        }
        f
    }

    fn lines(f: &[u8]) -> Result<Vec<RawLine>> {
        let v = parse_figure(f)?;
        let mut out = Vec::new();
        collect_lines(&v, &mut out);
        Ok(out)
    }

    #[test]
    fn reads_uncompressed_and_compressed() {
        for c in [false, true] {
            let l = lines(&sample_file(c)).unwrap();
            assert_eq!(l.len(), 1);
            assert_eq!(l[0].name, "sig");
            assert_eq!(l[0].x, [0.0, 1.0, 2.0]);
            assert_eq!(l[0].y, [5.0, 6.0, 7.0]);
        }
    }

    #[test]
    fn rejects_non_mat_files() {
        assert!(parse_figure(b"").is_err());
        assert!(parse_figure(&[0u8; 200]).is_err());
        let mut be = sample_file(false);
        be[126] = b'M';
        be[127] = b'I';
        assert!(parse_figure(&be).is_err());
    }

    #[test]
    fn every_truncation_is_handled_without_panic() {
        for c in [false, true] {
            let f = sample_file(c);
            for n in 0..f.len() {
                let _ = lines(&f[..n]);
            }
        }
    }

    #[test]
    fn truncated_data_gives_error_not_lines() {
        let f = sample_file(false);
        assert!(parse_figure(&f[..f.len() - 20]).is_err());
    }

    #[test]
    fn corrupted_bytes_never_panic() {
        // einfacher deterministischer Pseudozufall, kein extra Crate nötig
        let mut seed = 0x1234_5678_u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for c in [false, true] {
            let base = sample_file(c);
            for _ in 0..3000 {
                let mut f = base.clone();
                for _ in 0..1 + rnd() % 4 {
                    let i = 128 + rnd() as usize % (f.len() - 128);
                    f[i] = rnd() as u8;
                }
                let _ = lines(&f);
            }
        }
    }

    #[test]
    fn huge_dimensions_do_not_allocate() {
        let body = [elem(5, &[0xff, 0xff, 0xff, 0x7f, 0xff, 0xff, 0xff, 0x7f])];
        let mut m = elem(6, &[1, 0, 0, 0, 0, 0, 0, 0]); // Cell
        m.extend(&body[0]);
        m.extend(elem(1, b"hgS_x"));
        let mut f = header();
        f.extend(elem(14, &m));
        assert!(parse_figure(&f).is_err());
    }

    #[test]
    fn deep_nesting_is_rejected() {
        let mut inner = doubles("", &[1.0]);
        for _ in 0..200 {
            inner = matrix(1, &[1, 1], "", &[inner]);
        }
        let mut f = header();
        f.extend(inner);
        assert!(parse_figure(&f).is_err());
    }

    #[test]
    fn short_struct_field_length_element() {
        // Feldnamenlänge-Element mit 0 Byte darf nicht abstürzen
        let mut m = elem(6, &[2, 0, 0, 0, 0, 0, 0, 0]);
        m.extend(elem(5, &[1, 0, 0, 0, 1, 0, 0, 0]));
        m.extend(elem(1, b"hgS_x"));
        m.extend(elem(5, &[]));
        m.extend(elem(1, b""));
        let mut f = header();
        f.extend(elem(14, &m));
        assert!(parse_figure(&f).is_err());
    }

    #[test]
    fn empty_arrays_and_missing_fields() {
        let props = strukt("", &[("XData", matrix(6, &[0, 0], "", &[])), ("YData", doubles("", &[]))]);
        let line = strukt("", &[("type", text("", "line")), ("properties", props)]);
        let fig = strukt("hgS_1", &[("type", text("", "figure")), ("children", line)]);
        let mut f = header();
        f.extend(fig);
        let v = parse_figure(&f).unwrap();
        let mut out = Vec::new();
        collect_lines(&v, &mut out);
        assert!(out.len() <= 1);
    }

    #[test]
    fn unnamed_line_gets_default_name() {
        let props = strukt("", &[("XData", doubles("", &[0.0])), ("YData", doubles("", &[1.0]))]);
        let line = strukt("", &[("type", text("", "line")), ("properties", props)]);
        let fig = strukt("hgS_1", &[("type", text("", "figure")), ("children", line)]);
        let mut f = header();
        f.extend(fig);
        assert_eq!(lines(&f).unwrap()[0].name, "Signal 1");
    }
}
