// Compile-time host selection. This is the only cfg block callers ever see —
// every downstream module uses `os::Host::…` instead of a target_os cfg.
// An unsupported target OS fails with one readable compile_error! rather than
// a cascading "type Host not found" pile-up (see plan review issue #4).

#[cfg(target_os = "macos")]
pub type Host = super::macos::MacosHost;

#[cfg(target_os = "linux")]
pub type Host = super::linux::LinuxHost;

#[cfg(target_os = "windows")]
pub type Host = super::windows::WindowsHost;

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
compile_error!(
    "QuantaMind supports only macOS, Linux, and Windows — no os::Host adapter for this target OS."
);
