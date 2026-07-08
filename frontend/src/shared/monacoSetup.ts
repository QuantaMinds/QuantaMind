// Self-host Monaco (rules 7c/7e). `@monaco-editor/react` defaults to loading the ENTIRE
// editor from the jsDelivr CDN at runtime — that is remote code execution, it breaks the
// editor when the machine is offline (this is an offline-first app), and it blocks a strict
// `script-src 'self'` CSP. Bundling `monaco-editor` locally and pointing the loader at it
// removes all three: the editor and its web worker are then served from the app itself.
//
// Imported for its side effects from `main.tsx`, before any <Editor> mounts.
import * as monaco from "monaco-editor";
import { loader } from "@monaco-editor/react";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";

// The prompt editor is markdown/plaintext, so the base editor worker covers it — no
// json/ts/css/html language workers are needed.
self.MonacoEnvironment = {
  getWorker: (): Worker => new EditorWorker(),
};

loader.config({ monaco });
