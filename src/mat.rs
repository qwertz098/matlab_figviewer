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

fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
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
        if start + n > self.d.len() {
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

fn parse_matrix(data: &[u8]) -> Result<(String, Value)> {
    if data.is_empty() {
        // leeres Feld ([]) ist als miMATRIX mit 0 Byte abgelegt
        return Ok((String::new(), Value::Num(Vec::new())));
    }
    let mut r = Reader { d: data, pos: 0 };
    let (_, flags) = r.next()?;
    let class = flags[0];
    let (_, dims) = r.next()?;
    let n: usize = dims
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap()).max(0) as usize)
        .product();
    let (_, name) = r.next()?;
    let name = String::from_utf8_lossy(name).into_owned();
    let v = match class {
        1 => {
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                let (_, d) = r.next()?;
                items.push(parse_matrix(d)?.1);
            }
            Value::Cell(items)
        }
        2 | 3 => {
            if class == 3 {
                r.next()?; // Klassenname
            }
            let (_, l) = r.next()?;
            let len = u32le(l) as usize;
            let (_, nm) = r.next()?;
            let names: Vec<String> = nm
                .chunks(len.max(1))
                .map(|c| String::from_utf8_lossy(c).trim_end_matches('\0').to_string())
                .collect();
            let mut elems = Vec::with_capacity(n);
            for _ in 0..n {
                let mut row = Vec::with_capacity(names.len());
                for _ in 0..names.len() {
                    let (_, d) = r.next()?;
                    row.push(parse_matrix(d)?.1);
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
    let bytes = std::fs::read(path)?;
    if bytes.len() < 136 || !bytes.starts_with(b"MATLAB 5.0") {
        bail!("Keine MATLAB-v5-Datei (v7.3/HDF5 wird nicht unterstützt)");
    }
    if &bytes[126..128] != b"IM" {
        bail!("Big-Endian-Dateien werden nicht unterstützt");
    }
    let mut top = Reader { d: &bytes, pos: 128 };
    let mut last_err = String::new();
    while let Ok((ty, data)) = top.next() {
        let parsed = match ty {
            14 => parse_matrix(data),
            15 => {
                let mut buf = Vec::new();
                ZlibDecoder::new(data).read_to_end(&mut buf)?;
                let mut inner = Reader { d: &buf, pos: 0 };
                match inner.next() {
                    Ok((14, d)) => parse_matrix(d),
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
