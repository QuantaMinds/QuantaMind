//! Right-sizing synthesis: given the ranked per-model verdicts and the installed
//! registry's size metadata, name the smallest quant of each family that is
//! still Ready on THIS hardware, with measured percent reductions. Percent only
//! — no cost or currency anywhere (host-specific; never published). A concern
//! sub-folder so `readiness/` stays within the folder-taxonomy budget.
pub mod right_size;
