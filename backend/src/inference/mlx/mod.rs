pub mod mlx_backend;
pub mod server;

/// MLX (`mlx_lm.server`) only runs on Apple Silicon. Gates discovery, install,
/// and server start so those are no-ops/errors elsewhere. Centralized so the
/// `cfg!` lives in exactly one place.
pub fn mlx_supported() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}
