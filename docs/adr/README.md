# Architecture Decision Records

Short, dated, append-only notes capturing *why* a non-obvious decision was made — so
"why is this weird?" six months later is answered in the repo, not someone's memory
(see [`rust-engineering-architecture-guide.md`](../../rust-engineering-architecture-guide.md)
Part 8.5).

**Rules.** One decision per file, `NNNN-kebab-title.md`, numbered in order. Never edit
the decision of a past ADR — supersede it with a new one and link back. Keep each to
roughly one screen. Use [`0000-template.md`](0000-template.md).

## Index

| # | Decision | Status |
|---|---|---|
| [0001](0001-single-crate-not-workspace.md) | Single crate with modules-as-layers, not a Cargo workspace | Accepted |
| [0002](0002-enums-not-strings.md) | Closed discriminators (`BackendKind`, `ResponderKind`) are enums, not strings | Accepted |
| [0003](0003-versioned-collections.md) | An answer-key change mints a new versioned collection, never an in-place edit | Accepted |
| [0004](0004-tauri-free-domain.md) | `inference/` is Tauri-free; domain→IPC progress crosses via sink ports | Accepted |
| [0005](0005-no-fabricated-metrics.md) | Absence is `Option`/`N/A`, never a fabricated `0` | Accepted |
