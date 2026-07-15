use crate::inference::eval::agentic::sandbox::ResponderKind;
use crate::inference::eval::agentic::v2::env_webui::WebUiState;
use crate::inference::eval::toolcall::tasks::Call;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The per-turn environment snapshot for the visual replay — a PURE function of the
/// (immutable) responder and the turn's calls. Each environment picks its representative
/// action from the calls (the filesystem env uses the last read/list/search call, so a
/// `read_file` batched before a `reply` still shows the read). `EnvView::None` for
/// entity/static-mock tasks. The exhaustive match means adding a `ResponderKind` variant
/// forces a decision here. `web_ui` is the per-RUN mutable state (the only STATEFUL env): its
/// view must reflect the CURRENT (post-action) state, so the runner passes it in (the immutable
/// `ResponderKind::WebUi` holds only the spec). `None` when not a web-UI run.
pub fn env_view(responder: &ResponderKind, calls: &[Call], web_ui: Option<&WebUiState>) -> EnvView {
    match responder {
        ResponderKind::StaticMocks | ResponderKind::WorldState(_) => EnvView::None,
        ResponderKind::FileSystem(fs) => EnvView::FileSystem(fs.view(calls)),
        ResponderKind::WebCorpus(c) => EnvView::WebCorpus(c.view(calls)),
        ResponderKind::WebUi(_) => web_ui.map_or(EnvView::None, |st| EnvView::WebUi(st.view(calls))),
        // MCP: the real post-action view is built in the runner from the live `McpWorld`'s disk
        // (via `mcp_fsview`), like WebUi — this stateless path has no world, so None.
        ResponderKind::Mcp(_) => EnvView::None,
    }
}

/// Snapshot a REAL MCP world's sandbox dir into an `FsView` so the trace debugger renders it
/// exactly like a simulated filesystem turn. Walks `root` (bounded), derives the turn's
/// focus/op from the last path-bearing call.
pub fn mcp_fsview(root: &std::path::Path, calls: &[Call]) -> FsView {
    let mut tree = Vec::new();
    walk_dir(root, root, &mut tree, 0);
    tree.sort_by(|a, b| a.path.cmp(&b.path));
    let focus_path = calls.iter().rev().find_map(|c| {
        c.args.get("path").and_then(|v| v.as_str()).map(|p| {
            std::path::Path::new(p)
                .strip_prefix(root)
                .map(|r| r.to_string_lossy().into_owned())
                .unwrap_or_else(|_| p.to_string())
        })
    });
    let op = calls.last().map(|c| fs_op_for(&c.name)).unwrap_or(FsOp::None);
    FsView { tree, focus_path, op, content: None, matches: vec![] }
}

fn walk_dir(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<FsNode>, depth: usize) {
    if depth > 8 || out.len() > 500 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let is_dir = path.is_dir();
        if let Ok(rel) = path.strip_prefix(base) {
            out.push(FsNode { path: rel.to_string_lossy().into_owned(), is_dir });
        }
        if is_dir {
            walk_dir(base, &path, out, depth + 1);
        }
    }
}

fn fs_op_for(name: &str) -> FsOp {
    let n = name.rsplit("::").next().unwrap_or(name);
    if n.starts_with("read") {
        FsOp::Read
    } else if n.starts_with("list") || n == "directory_tree" {
        FsOp::List
    } else if n.starts_with("search") {
        FsOp::Search
    } else {
        FsOp::None
    }
}

/// A per-turn snapshot of the deterministic environment the agent acted on, streamed
/// alongside each [`crate::inference::eval::agentic::step::TrajectoryStep`] so the UI can
/// **visually replay** the run (file tree, later search results / web-UI state). It is a PURE
/// function of `(immutable env state, call)` — environments are stateless-per-call — so the
/// picture can never disagree with the score.
///
/// Internal / local-only: `EnvView` is the agent's run (= model output) and is **NEVER** added
/// to the publish allowlist (`persistence::publish`). The watchable replay is a local
/// experience; the leaderboard ships metrics only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnvView {
    /// No environment (entity/world_state and static-mock tasks) — the UI renders only the
    /// text trace, no replay panel.
    #[default]
    None,
    /// A simulated-filesystem turn (Phase 1).
    FileSystem(FsView),
    /// A frozen web-search-corpus turn (Phase 2): a `search` or `fetch` over a bundled corpus.
    WebCorpus(CorpusView),
    /// A stateful web-UI turn (Phase 2, Slice 3): the CURRENT (post-action) UI state + the action
    /// the agent took this turn. The state MUTATES across turns (fill/click/navigate).
    WebUi(WebUiView),
}

/// What the frozen search corpus looked like and what the agent did this turn. Lazy by design:
/// `index` carries only `doc_id`+`title` for every doc (the corpus can be large); the full text
/// rides along ONLY for the one doc the agent `fetch`ed this turn (`content`). A PURE function of
/// (immutable corpus, calls).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CorpusView {
    /// The corpus index — every doc's id + title, sorted by id (NOT full bodies — lazy load).
    pub index: Vec<CorpusDoc>,
    /// The search query this turn, if the agent searched.
    pub query: Option<String>,
    /// Ranked search hits for `query` (deterministic), each with a snippet.
    pub results: Vec<CorpusHit>,
    /// The doc the agent `fetch`ed this turn — highlighted in the reader. `None` if it searched.
    pub focus_doc: Option<String>,
    /// For a `fetch`: the returned full document text (the real content — never an empty ack).
    pub content: Option<String>,
    /// Which corpus operation the agent performed this turn.
    pub op: CorpusOp,
}

/// One entry in the corpus index (id + title only — the lazy index).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CorpusDoc {
    pub doc_id: String,
    pub title: String,
}

/// One ranked search hit: the doc + a deterministic snippet (first query-term line, else head).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CorpusHit {
    pub doc_id: String,
    pub title: String,
    pub snippet: String,
}

/// The corpus operation a turn performed (drives the panel's highlight + which detail to show).
/// `None` = a recognized call that didn't map to a corpus op (e.g. `reply`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CorpusOp {
    #[default]
    None,
    Search,
    Fetch,
}

/// What the simulated filesystem looked like and what the agent did to it this turn. The
/// `tree` is small for coding tasks, so slice 1 carries it per-turn for simplicity (a later
/// optimization can ship the static tree once and stream only the delta).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FsView {
    /// The (static) file tree, sorted by path for a stable render.
    pub tree: Vec<FsNode>,
    /// The path the agent touched this turn — highlighted in the panel. `None` if the turn's
    /// call didn't resolve to a path.
    pub focus_path: Option<String>,
    /// Which filesystem operation the agent performed this turn.
    pub op: FsOp,
    /// For a `read_file`: the returned file content (the real content — never an empty ack).
    pub content: Option<String>,
    /// For a `list_dir` / `search_files` / `grep`: the matched paths or lines.
    pub matches: Vec<String>,
}

/// One node in the rendered file tree.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FsNode {
    pub path: String,
    pub is_dir: bool,
}

/// What the web UI looked like AFTER this turn's action(s) + which control the agent touched. The
/// `state` is the full (small) UI state machine (routes/fields/toggles/submitted), rendered as a
/// schematic in the replay. A pure function of the per-run state + the turn's calls.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WebUiView {
    /// The current (post-action) UI state — the schematic the replay renders.
    pub state: Value,
    /// The UI action the agent took this turn (`fill`/`click`/`navigate`/…). `None` if no UI action.
    pub action: Option<String>,
    /// The widget/field/route the action touched — highlighted in the schematic.
    pub focus: Option<String>,
}

/// The filesystem operation a turn performed (drives the panel's highlight + which detail
/// to show). `None` = a recognized call that didn't map to a filesystem op (e.g. `reply`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FsOp {
    #[default]
    None,
    Read,
    List,
    Search,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn none_serializes_as_tagged_object_matching_the_zod_schema() {
        // EnvView::None → {"kind":"none"} (internally tagged). The frontend Zod
        // discriminatedUnion("kind", …) expects exactly this.
        assert_eq!(serde_json::to_value(EnvView::None).unwrap(), json!({ "kind": "none" }));
    }

    #[test]
    fn filesystem_flattens_fsview_next_to_the_kind_tag() {
        // Internally-tagged newtype variant flattens FsView's fields beside "kind", which the
        // Zod file_system branch (tree/focus_path/op/content/matches) parses.
        let v = EnvView::FileSystem(FsView {
            tree: vec![FsNode { path: "config.yaml".into(), is_dir: false }],
            focus_path: Some("config.yaml".into()),
            op: FsOp::Read,
            content: Some("timeout: 30".into()),
            matches: vec![],
        });
        assert_eq!(
            serde_json::to_value(v).unwrap(),
            json!({
                "kind": "file_system",
                "tree": [{ "path": "config.yaml", "is_dir": false }],
                "focus_path": "config.yaml",
                "op": "read",
                "content": "timeout: 30",
                "matches": []
            })
        );
    }

    #[test]
    fn web_corpus_flattens_corpusview_next_to_the_kind_tag() {
        // Internally-tagged newtype variant flattens CorpusView beside "kind", which the Zod
        // web_corpus branch (index/query/results/focus_doc/content/op) parses.
        let v = EnvView::WebCorpus(CorpusView {
            index: vec![CorpusDoc { doc_id: "d1".into(), title: "Photosynthesis".into() }],
            query: Some("light".into()),
            results: vec![CorpusHit { doc_id: "d1".into(), title: "Photosynthesis".into(), snippet: "uses light".into() }],
            focus_doc: None,
            content: None,
            op: CorpusOp::Search,
        });
        assert_eq!(
            serde_json::to_value(v).unwrap(),
            json!({
                "kind": "web_corpus",
                "index": [{ "doc_id": "d1", "title": "Photosynthesis" }],
                "query": "light",
                "results": [{ "doc_id": "d1", "title": "Photosynthesis", "snippet": "uses light" }],
                "focus_doc": null,
                "content": null,
                "op": "search"
            })
        );
    }

    #[test]
    fn web_ui_flattens_webuiview_next_to_the_kind_tag() {
        // Internally-tagged newtype variant flattens WebUiView beside "kind", which the Zod
        // web_ui branch (state/action/focus) parses.
        let v = EnvView::WebUi(WebUiView {
            state: json!({ "route": "/cart", "submitted": true }),
            action: Some("submit".into()),
            focus: Some("checkout".into()),
        });
        assert_eq!(
            serde_json::to_value(v).unwrap(),
            json!({
                "kind": "web_ui",
                "state": { "route": "/cart", "submitted": true },
                "action": "submit",
                "focus": "checkout"
            })
        );
    }

    #[test]
    fn default_env_is_none() {
        assert_eq!(EnvView::default(), EnvView::None);
    }
}
