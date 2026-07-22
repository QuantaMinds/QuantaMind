// Speech-to-text (whisper.cpp) domain logic: model-format validation, the
// curated catalog, and the offline reachability seam. Pure/domain — holds no
// AppHandle and never imports `crate::commands` (see docs/architecture.md#layering).
pub mod eval;
// Audio-crate consumers (cpal/webrtc-vad/symphonia/hound/rubato) are GUI-only:
// the headless CLI scores STORED transcripts (eval/) and never decodes audio.
#[cfg(feature = "gui")]
pub mod profile;
pub mod stt_catalog;
pub mod stt_format;
pub mod stt_probe;
pub mod transcribe;
