use crate::errors::AppError;
use crate::inference::gguf::gguf_reader::GgufReader;

#[derive(Debug, Clone, PartialEq)]
pub enum GgufValue {
    U8(u8), I8(i8), U16(u16), I16(i16),
    U32(u32), I32(i32), F32(f32), Bool(bool),
    String(String), U64(u64), I64(i64), F64(f64),
    // A small INT array, kept (as i64) because some metadata is stored per-block —
    // e.g. gemma's `attention.head_count_kv` is one value per layer. String/float/
    // nested and oversized (vocab-sized) arrays are still drained without keeping
    // values (`ArraySkipped`), since callers only inspect scalar/small-int metadata.
    IntArray(Vec<i64>),
    ArraySkipped,
}

/// Cap on captured `IntArray` length — per-block arrays (layer count) are tiny; this
/// keeps a vocab-sized int array (e.g. `tokenizer.ggml.token_type`) from being held.
const MAX_CAPTURED_ARRAY: usize = 1024;

/// Integer element tags (excludes bool/float/string) — the only arrays worth keeping.
fn is_int_tag(tag: u32) -> bool {
    matches!(tag, 0..=5 | 10 | 11)
}

/// Read one integer array element of `tag` into `i64`. Caller guarantees `is_int_tag`.
fn read_int_elem(r: &mut GgufReader, tag: u32) -> Result<i64, AppError> {
    Ok(match tag {
        0 => r.u8()? as i64,
        1 => (r.u8()? as i8) as i64,
        2 => r.u16()? as i64,
        3 => r.i16()? as i64,
        4 => r.u32()? as i64,
        5 => r.i32()? as i64,
        10 => r.u64()? as i64,
        _ => r.i64()?, // tag 11
    })
}

fn checked_count(n64: u64) -> Result<usize, AppError> {
    usize::try_from(n64).map_err(|_| AppError::Validation(
        format!("GGUF array count {n64} exceeds usize on this platform")
    ))
}

fn skip_value(r: &mut GgufReader, tag: u32) -> Result<(), AppError> {
    match tag {
        0 | 1 | 7 => { r.u8()?; }
        2 | 3 => { r.u16()?; }
        4..=6 => { r.u32()?; }
        8 => { r.string()?; }
        9 => {
            let elem = r.u32()?;
            let n = checked_count(r.u64()?)?;
            for _ in 0..n { skip_value(r, elem)?; }
        }
        10..=12 => { r.u64()?; }
        _ => return Err(AppError::Validation(format!("unknown GGUF value tag: {tag}"))),
    }
    Ok(())
}

#[cfg(test)]
#[path = "gguf_value_tests.rs"]
mod tests;

pub fn read_value(r: &mut GgufReader) -> Result<GgufValue, AppError> {
    let tag = r.u32()?;
    match tag {
        0 => Ok(GgufValue::U8(r.u8()?)),
        1 => Ok(GgufValue::I8(r.u8()? as i8)),
        2 => Ok(GgufValue::U16(r.u16()?)),
        3 => Ok(GgufValue::I16(r.i16()?)),
        4 => Ok(GgufValue::U32(r.u32()?)),
        5 => Ok(GgufValue::I32(r.i32()?)),
        6 => Ok(GgufValue::F32(r.f32()?)),
        7 => Ok(GgufValue::Bool(r.u8()? != 0)),
        8 => Ok(GgufValue::String(r.string()?)),
        9 => {
            let elem = r.u32()?;
            let n = checked_count(r.u64()?)?;
            // Keep a small int array (e.g. gemma's per-layer head_count_kv); otherwise
            // drain without retaining (strings/floats/nested/oversized).
            if is_int_tag(elem) && n <= MAX_CAPTURED_ARRAY {
                let mut out = Vec::with_capacity(n);
                for _ in 0..n { out.push(read_int_elem(r, elem)?); }
                Ok(GgufValue::IntArray(out))
            } else {
                for _ in 0..n { skip_value(r, elem)?; }
                Ok(GgufValue::ArraySkipped)
            }
        }
        10 => Ok(GgufValue::U64(r.u64()?)),
        11 => Ok(GgufValue::I64(r.i64()?)),
        12 => Ok(GgufValue::F64(r.f64()?)),
        _ => Err(AppError::Validation(format!("unknown GGUF value tag: {tag}"))),
    }
}
