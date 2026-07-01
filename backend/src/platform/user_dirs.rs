use std::path::PathBuf;

/// The per-user data directory QuantaMind writes into.
///
/// * Unix (macOS + Linux) → `~/.quantamind` — byte-identical to every existing
///   install, so this refactor is backwards compatible.
/// * Windows → `%LOCALAPPDATA%\QuantaMind` — the native location a fresh
///   install should use instead of forcing users to set `QUANTAMIND_GGUF_DIR`.
///
/// Fallback (rare — home dir unresolvable) is a bare relative path; the
/// existing `storage_disk::absolutize()` anchors it onto `current_dir()`.
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .map(|p| p.join("QuantaMind"))
            .unwrap_or_else(|| PathBuf::from("QuantaMind"))
    }
    #[cfg(not(windows))]
    {
        dirs::home_dir()
            .map(|h| h.join(".quantamind"))
            .unwrap_or_else(|| PathBuf::from(".quantamind"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_ends_with_expected_leaf() {
        let d = data_dir();
        let leaf = d.file_name().and_then(|s| s.to_str()).unwrap_or("");
        #[cfg(windows)]
        assert_eq!(leaf, "QuantaMind");
        #[cfg(not(windows))]
        assert_eq!(leaf, ".quantamind");
    }
}
