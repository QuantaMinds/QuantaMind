use crate::errors::AppError;
use crate::inference::gguf::gguf_family::family_from_architecture;
use crate::inference::gguf::gguf_quant::{file_type_to_quant, quant_from_filename};
use crate::inference::gguf::gguf_reader::GgufReader;
use crate::inference::gguf::gguf_value::{read_value, GgufValue};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

// `Read::read` on macOS only returns one syscall's worth (~64KB),
// so we use `take(N).read_to_end()` to actually fill the window.
// 8 MiB covers most tokenizers; large-vocab BPE metadata (e.g. Qwen3)
// runs past it, so `inspect_gguf` grows the window on truncation up to
// `MAX_HEADER_READ_BYTES` rather than failing on a valid file.
const HEADER_READ_BYTES: u64 = 8 * 1024 * 1024;
const MAX_HEADER_READ_BYTES: u64 = 256 * 1024 * 1024;
const MIN_FILE_SIZE: u64 = 64 * 1024;

#[derive(Serialize, Clone, Debug)]
pub struct GgufMetadata {
    pub architecture: String,
    pub parameter_count: Option<u64>,
    pub context_length: Option<u32>,
    pub quantization: Option<String>,
    pub family: String,
    /// Transformer dims for the KV-cache size estimate (`vram_math`), each read from
    /// the `<arch>.*` header keys and `None` when the header omits one. Used by the
    /// llama-server spawn to bound `-c` to what RAM holds; absent ⇒ the spawn falls
    /// back to its safe default rather than guessing.
    pub block_count: Option<u64>,
    pub head_count: Option<u64>,
    pub head_count_kv: Option<u64>,
    pub embedding_length: Option<u64>,
}

fn as_string(v: &GgufValue) -> Option<&str> {
    if let GgufValue::String(s) = v { Some(s.as_str()) } else { None }
}

fn as_u64(v: &GgufValue) -> Option<u64> {
    match v {
        GgufValue::U64(n) => Some(*n),
        GgufValue::U32(n) => Some(*n as u64),
        GgufValue::I64(n) if *n >= 0 => Some(*n as u64),
        GgufValue::I32(n) if *n >= 0 => Some(*n as u64),
        _ => None,
    }
}

pub fn inspect_gguf_bytes(bytes: &[u8]) -> Result<GgufMetadata, AppError> {
    let mut r = GgufReader::new(bytes);
    r.magic(b"GGUF")?;
    let version = r.u32()?;
    if !(1..=3).contains(&version) {
        return Err(AppError::Validation(format!("unsupported GGUF version: {version}")));
    }
    let _tensor_count = r.u64()?;
    let kv_count = r.u64()?;
    let mut kv: HashMap<String, GgufValue> = HashMap::with_capacity(kv_count as usize);
    for _ in 0..kv_count {
        let key = r.string()?;
        let value = read_value(&mut r)?;
        kv.insert(key, value);
    }

    let architecture = kv.get("general.architecture")
        .and_then(as_string).unwrap_or("").to_string();
    let parameter_count = kv.get("general.parameter_count").and_then(as_u64);
    let context_length = kv
        .get(&format!("{architecture}.context_length"))
        .and_then(as_u64)
        .map(|n| n.min(u32::MAX as u64) as u32);
    let quantization = kv
        .get("general.file_type")
        .and_then(|v| match v {
            GgufValue::U32(n) => Some(*n),
            _ => None,
        })
        .and_then(file_type_to_quant)
        .map(|s| s.to_string());
    let family = family_from_architecture(&architecture);
    let dim = |suffix: &str| kv.get(&format!("{architecture}.{suffix}")).and_then(as_u64);
    let block_count = dim("block_count");
    let head_count = dim("attention.head_count");
    let head_count_kv = dim("attention.head_count_kv");
    let embedding_length = dim("embedding_length");

    Ok(GgufMetadata {
        architecture,
        parameter_count,
        context_length,
        quantization,
        family,
        block_count,
        head_count,
        head_count_kv,
        embedding_length,
    })
}

fn read_prefix(path: &Path, n: u64) -> Result<Vec<u8>, AppError> {
    let f = fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    let mut buf: Vec<u8> = Vec::with_capacity(n as usize);
    f.take(n).read_to_end(&mut buf).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(buf)
}

pub fn inspect_gguf(path: &Path) -> Result<GgufMetadata, AppError> {
    let ext_ok = path.extension().and_then(|e| e.to_str())
        .map(|s| s.eq_ignore_ascii_case("gguf")).unwrap_or(false);
    if !ext_ok {
        return Err(AppError::Validation(format!("not a .gguf file: {}", path.display())));
    }
    let md = fs::metadata(path).map_err(|e| AppError::Io(e.to_string()))?;
    if md.len() < MIN_FILE_SIZE {
        return Err(AppError::Validation(format!(
            "file too small to be a real GGUF: {} bytes", md.len()
        )));
    }
    // Read a prefix and parse; if the metadata runs past the window (Truncated),
    // double the window and retry. Tensor data follows the KV block, so the
    // metadata is always within the first `ceiling` bytes of a valid file —
    // once the window reaches `ceiling`, a Truncated error means a genuinely
    // incomplete file, and we surface it.
    let ceiling = md.len().min(MAX_HEADER_READ_BYTES);
    let mut window = HEADER_READ_BYTES.min(ceiling);
    let mut meta = loop {
        let buf = read_prefix(path, window)?;
        match inspect_gguf_bytes(&buf) {
            Ok(m) => break m,
            Err(AppError::Truncated(_)) if window < ceiling => {
                window = window.saturating_mul(2).min(ceiling);
            }
            Err(e) => return Err(e),
        }
    };
    if meta.quantization.is_none() {
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            meta.quantization = quant_from_filename(name);
        }
    }
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv_key(key: &str) -> Vec<u8> {
        let mut b = (key.len() as u64).to_le_bytes().to_vec();
        b.extend_from_slice(key.as_bytes());
        b
    }
    fn kv_string(key: &str, val: &str) -> Vec<u8> {
        let mut b = kv_key(key);
        b.extend_from_slice(&8u32.to_le_bytes()); // tag: string
        b.extend_from_slice(&(val.len() as u64).to_le_bytes());
        b.extend_from_slice(val.as_bytes());
        b
    }
    fn kv_u32(key: &str, val: u32) -> Vec<u8> {
        let mut b = kv_key(key);
        b.extend_from_slice(&4u32.to_le_bytes()); // tag: u32
        b.extend_from_slice(&val.to_le_bytes());
        b
    }
    fn gguf(kvs: &[Vec<u8>]) -> Vec<u8> {
        let mut b = b"GGUF".to_vec();
        b.extend_from_slice(&3u32.to_le_bytes()); // version
        b.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        b.extend_from_slice(&(kvs.len() as u64).to_le_bytes());
        kvs.iter().for_each(|kv| b.extend_from_slice(kv));
        b
    }

    /// The four `<arch>.*` dims the hardware-ceiling KV estimate needs parse from a
    /// real header — Llama-3-8B's values (32 layers, 32 heads, 8 KV heads, 4096 emb).
    #[test]
    fn parses_kv_dims_for_the_hardware_ceiling() {
        let bytes = gguf(&[
            kv_string("general.architecture", "llama"),
            kv_u32("llama.context_length", 8192),
            kv_u32("llama.block_count", 32),
            kv_u32("llama.attention.head_count", 32),
            kv_u32("llama.attention.head_count_kv", 8),
            kv_u32("llama.embedding_length", 4096),
        ]);
        let m = inspect_gguf_bytes(&bytes).expect("valid gguf");
        assert_eq!(m.block_count, Some(32));
        assert_eq!(m.head_count, Some(32));
        assert_eq!(m.head_count_kv, Some(8));
        assert_eq!(m.embedding_length, Some(4096));
    }

    /// A header that omits the dims yields `None` for each (the degrade path the
    /// ceiling treats as "can't size → safe default"), not a zero or a panic.
    #[test]
    fn kv_dims_are_none_when_the_header_omits_them() {
        let bytes = gguf(&[
            kv_string("general.architecture", "llama"),
            kv_u32("llama.context_length", 8192),
        ]);
        let m = inspect_gguf_bytes(&bytes).expect("valid gguf");
        assert_eq!(m.block_count, None);
        assert_eq!(m.head_count, None);
        assert_eq!(m.head_count_kv, None);
        assert_eq!(m.embedding_length, None);
    }
}
