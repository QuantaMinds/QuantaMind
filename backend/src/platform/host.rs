// Compile-time host selection. This is the only cfg block callers ever see —
// every downstream module uses `platform::Host::…` instead of a target_os cfg.

#[cfg(unix)]
pub type Host = super::unix_host::UnixHost;

#[cfg(windows)]
pub type Host = super::windows_host::WindowsHost;
