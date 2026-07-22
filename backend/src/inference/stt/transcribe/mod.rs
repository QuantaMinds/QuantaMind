// P1 transcription seam (whisper.cpp): decode audio in Rust → resample to 16 kHz
// → window → call whisper-server /inference per window → stream segments through
// a sink → assemble a canonical Transcript. Tauri-free (streaming via the
// TranscribeSink trait); strictly offline (loopback-only).
#[cfg(feature = "gui")]
pub mod audio;
#[cfg(feature = "gui")]
pub mod backend;
#[cfg(feature = "gui")]
pub mod decode_mp3;
pub mod dedup;
pub mod sink;
pub mod transcript;
#[cfg(feature = "gui")]
pub mod whisper_cpp;
