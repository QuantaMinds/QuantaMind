# Splice Project — Complete Beginner's Guide

## Table of Contents
1. [What is Splice?](#what-is-splice)
2. [Project Overview](#project-overview)
3. [Tech Stack Explained](#tech-stack-explained)
4. [Folder Structure](#folder-structure)
5. [How the Code Works](#how-the-code-works)
6. [Language Guide: Rust](#language-guide-rust)
7. [Language Guide: TypeScript](#language-guide-typescript)
8. [Workflow Explained](#workflow-explained)
9. [Development Setup](#development-setup)
10. [Common Tasks](#common-tasks)

---

## What is Splice?

**Splice** is a desktop application that lets you run AI models locally on your computer. Think of it as a mini version of ChatGPT that runs entirely on your machine, not in the cloud.

### Key Features
- Run AI models locally (no internet needed after setup)
- Edit prompts and see AI responses in real-time
- Save and load your prompts
- Compare different AI models side-by-side
- Track performance metrics (how fast it responds, how many tokens per second)

### Why This Project?
This is a **full-stack desktop application** built with:
- **Frontend**: React (web UI) + TypeScript
- **Backend**: Rust (fast, safe, native code)
- **Desktop Framework**: Tauri (runs on Windows, Mac, Linux)
- **AI Backend**: llama.cpp (runs models locally)

---

## Project Overview

### The Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Splice Desktop App                       │
│                                                             │
│  ┌────────────────────────────────────────────────────┐    │
│  │            React + TypeScript Frontend             │    │
│  │  features/  ←  shared/ipc/  ←  Tauri invoke()      │    │
│  └──────────────────────────┬─────────────────────────┘    │
│                             │                              │
│                    IPC boundary (JSON)                     │
│                             │                              │
│  ┌──────────────────────────▼─────────────────────────┐    │
│  │               Rust Backend (backend/)              │    │
│  │  commands/  →  inference/  →  metrics/             │    │
│  │       ↓                                            │    │
│  │  persistence/                                      │    │
│  └──────────────────────────┬─────────────────────────┘    │
└─────────────────────────────┼──────────────────────────────┘
                              │ HTTP
                              ▼
                ┌─────────────────────────────┐
                │   llama.cpp (localhost:8081)  │
                └─────────────────────────────┘
```

### How Data Flows

1. **User types in the UI** → React component updates state
2. **User clicks "Run"** → React calls Tauri's `invoke()` function
3. **Tauri sends JSON message** → Rust backend receives it
4. **Rust validates input** → Checks if it's valid
5. **Rust talks to llama.cpp** → Sends HTTP request to localhost:8081
6. **llama.cpp runs the model** → Returns tokens one by one
7. **Rust streams tokens back** → Sends JSON messages to frontend
8. **React displays tokens** → Updates the UI in real-time

---

## Tech Stack Explained

### 1. Tauri 2.x — Desktop Framework
**What it is**: A framework to build desktop apps with web technologies
**Why use it**:
- Smaller than Electron (30MB vs 150MB)
- Uses native OS features (file system, notifications)
- Runs Rust backend for speed and security
- Cross-platform (Windows, Mac, Linux)

**Key Concept**: Tauri acts as a bridge between web (React) and native (Rust)

### 2. Rust — Backend Language
**What it is**: A systems programming language (like C++ but safer)
**Why use it**:
- Extremely fast (close to C/C++ performance)
- Memory-safe (no crashes from memory bugs)
- Great for network operations and file handling
- Compiles to native machine code

**Key Concept**: Rust runs on your computer, not in the browser

### 3. React 18 + TypeScript — Frontend
**What it is**: A JavaScript library for building user interfaces
**Why use it**:
- Popular, lots of learning resources
- Great for interactive UIs
- TypeScript adds type safety (catches bugs before runtime)

**Key Concept**: React builds the visual interface you see

### 4. Vite — Build Tool
**What it is**: A tool that compiles your code and starts a dev server
**Why use it**:
- Very fast (instant updates when you save)
- Optimizes code for production
- Works great with React and TypeScript

**Key Concept**: Vite takes your code and makes it run in the browser

### 5. Tailwind CSS — Styling
**What it is**: A utility-first CSS framework
**Why use it**:
- No need to write custom CSS classes
- Quick to prototype designs
- Consistent look across the app

**Key Concept**: Tailwind gives you pre-built CSS classes like `text-blue-500`, `p-4`, `flex`

### 6. Zustand — State Management
**What it is**: A tiny library for managing app state
**Why use it**:
- Simple API (no complex setup)
- Smaller than Redux
- Perfect for this project's needs

**Key Concept**: Zustand stores data that multiple components need to access

### 7. Zod — Validation
**What it is**: A runtime type validation library
**Why use it**:
- Validates data before it goes to the backend
- TypeScript-like validation in JavaScript
- Catches errors early

**Key Concept**: Zod checks if data is in the right format before processing

### 8. llama.cpp — AI Model Runner
**What it is**: A tool that runs AI models locally
**Why use it**:
- Free and open-source
- Runs models on your CPU/GPU
- Simple HTTP API

**Key Concept**: llama.cpp is the "brain" that actually runs the AI models

---

## Folder Structure

```
QM-Dev/
├── .github/                          # GitHub configuration
│   ├── workflows/                    # CI/CD pipelines
│   │   ├── ci.yml                    # Continuous integration
│   │   ├── release.yml               # Release automation
│   │   └── nightly.yml               # Nightly builds
│   └── PULL_REQUEST_TEMPLATE.md      # PR template
│
├── frontend/                         # React + TypeScript app
│   ├── src/
│   │   ├── app/                      # Application shell
│   │   │   ├── App.tsx               # Main React component
│   │   │   ├── routes.tsx            # Routing configuration
│   │   │   └── providers.tsx         # React providers
│   │   │
│   │   ├── features/                 # Feature modules (vertical slices)
│   │   │   ├── workspace/            # Phase 1: Main workspace
│   │   │   │   ├── components/       # UI components
│   │   │   │   │   ├── PromptEditor.tsx      # Text input for prompts
│   │   │   │   │   ├── OutputStream.tsx       # Displays AI responses
│   │   │   │   │   ├── ModelPicker.tsx        # Select AI models
│   │   │   │   │   ├── RunControls.tsx        # Run/Cancel buttons
│   │   │   │   │   └── WorkspaceIO.tsx        # Save/Load prompts
│   │   │   │   ├── hooks/            # Custom React hooks
│   │   │   │   │   ├── useStreamingRun.ts     # Handle streaming output
│   │   │   │   │   └── usePromptStore.ts      # Local state management
│   │   │   │   ├── state/            # Zustand store
│   │   │   │   │   └── workspaceStore.ts
│   │   │   │   ├── types.ts          # TypeScript types
│   │   │   │   ├── schemas.ts        # Zod validation schemas
│   │   │   │   └── __tests__/        # Component tests
│   │   │   │
│   │   │   ├── models/               # Phase M: Model management
│   │   │   ├── inspector/            # Phase 4: Performance inspector
│   │   │   ├── bench/                # Phase 3: Model comparison
│   │   │   └── settings/             # Phase 2: Settings
│   │   │
│   │   ├── shared/                   # Shared code between features
│   │   │   ├── components/           # Reusable components
│   │   │   │   └── common/           # Shared UI elements
│   │   │   ├── ipc/                  # Inter-Process Communication
│   │   │   │   ├── client.ts         # Tauri invoke wrapper
│   │   │   │   └── types.ts          # TypeScript IPC types
│   │   │   └── styles/               # Global styles
│   │   │       └── tokens.css        # Design tokens
│   │   │
│   │   ├── test/                     # Test setup
│   │   │   └── setup.ts              # Test configuration
│   │   ├── main.tsx                  # React entry point
│   │   └── index.css                 # Global CSS
│   │
│   ├── index.html                    # HTML template
│   ├── package.json                  # Frontend dependencies
│   ├── pnpm-lock.yaml                # Dependency lock file
│   ├── tsconfig.json                 # TypeScript configuration
│   ├── tsconfig.node.json            # TypeScript for Node
│   ├── vite.config.ts                # Vite build configuration
│   ├── vitest.config.ts              # Vitest test configuration
│   ├── tailwind.config.js            # Tailwind CSS configuration
│   └── postcss.config.js             # PostCSS configuration
│
├── backend/                          # Rust + Tauri backend
│   ├── src/
│   │   ├── main.rs                   # Rust entry point
│   │   ├── lib.rs                    # Library code
│   │   │
│   │   ├── commands/                 # Tauri commands (IPC handlers)
│   │   │   ├── mod.rs                # Module exports
│   │   │   ├── prompt.rs             # Handle prompt commands
│   │   │   ├── models.rs             # Handle model commands
│   │   │   ├── settings.rs           # Handle settings commands
│   │   │   └── workspace.rs          # Handle workspace commands
│   │   │
│   │   ├── inference/                # AI model inference
│   │   │   ├── mod.rs                # Module exports
│   │   │   ├── llama_cpp.rs             # llama.cpp backend
│   │   │   ├── llama_cpp.rs          # Llama.cpp backend
│   │   │   ├── vllm.rs                # vLLM (Apple Silicon) backend
│   │   │   └── traits.rs             # Backend trait definitions
│   │   │
│   │   ├── metrics/                  # Performance metrics
│   │   │   ├── mod.rs                # Module exports
│   │   │   ├── timing.rs             # Time tracking
│   │   │   └── vram.rs               # VRAM usage tracking
│   │   │
│   │   ├── persistence/              # Data persistence
│   │   │   ├── mod.rs                # Module exports
│   │   │   ├── prompts.rs            # Prompt file handling
│   │   │   └── history.rs            # Run history tracking
│   │   │
│   │   ├── validation/               # Input validation
│   │   │   ├── mod.rs                # Module exports
│   │   │   └── schemas.rs            # Validation schemas
│   │   │
│   │   └── errors.rs                 # Error handling
│   │
│   ├── tests/                        # Rust integration tests
│   │   ├── llama_cpp_stream.rs          # Test llama.cpp streaming
│   │   ├── models_list.rs            # Test model listing
│   │   └── prompt_stream.rs          # Test prompt streaming
│   │
│   ├── Cargo.toml                    # Rust dependencies
│   ├── tauri.conf.json               # Tauri configuration
│   ├── build.rs                      # Build script
│   ├── capabilities/                 # Tauri capabilities
│   └── icons/                        # App icons
│
├── docs/                             # Project documentation
│   ├── architecture.md               # Architecture overview
│   ├── tech-stack.md                 # Tech stack decisions
│   ├── folder-structure.md           # Folder structure rationale
│   ├── setup.md                      # Setup instructions
│   ├── conventions.md                # Coding conventions
│   ├── workflow.md                   # Development workflow
│   ├── data-quality.md               # Data quality checks
│   ├── phase-roadmap.md              # Project phases
│   └── future-considerations.md      # Future ideas
│
├── CLAUDE.md                         # Project guide for Claude Code
├── README.md                         # Project README
├── LICENSE                           # License
└── CHANGELOG.md                      # Version history
```

### Why This Structure?

1. **`frontend/` + `backend/` top split** — Two languages, two toolchains. Co-locating configs with source means each side is self-contained.

2. **`features/` over `components/`** — Each feature is a vertical slice: components + hooks + state + tests. Deletable in one `rm -rf`.

3. **`commands/` mirrors `features/`** — Every command corresponds to a frontend need.

4. **`__tests__/` next to code** — Tests live next to what they test.

---

## How the Code Works

### Frontend Flow (React + TypeScript)

```typescript
// 1. User types in PromptEditor component
<PromptEditor value={prompt} onChange={setPrompt} />

// 2. State updates in App.tsx
const [prompt, setPrompt] = useState("");

// 3. User clicks "Run" button
<RunControls onRun={() => model && start(model, prompt)} />

// 4. Custom hook handles the streaming
const { output, status, error, metrics, start, cancel } =
  useStreamingRun();

// 5. Hook calls Tauri to send request to backend
const start = async (model: string, prompt: string) => {
  const result = await invoke('run_prompt', {
    model,
    prompt
  });
  // Handle result...
};

// 6. Output is displayed in OutputStream
<OutputStream output={output} />
```

### Backend Flow (Rust)

```rust
// 1. Tauri command receives request
#[tauri::command]
pub async fn run_prompt(
    model: String,
    prompt: String,
) -> Result<String, AppError> {
    // 2. Validate input
    let prompt = validate_prompt(&prompt)?;

    // 3. Call inference backend
    let output = InferenceBackend::run(&model, &prompt).await?;

    // 4. Return result
    Ok(output)
}

// 5. Inference backend talks to llama.cpp
pub async fn run(model: &str, prompt: &str) -> Result<String, AppError> {
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:8081/api/generate")
        .json(&json!({
            "model": model,
            "prompt": prompt,
            "stream": true,
        }))
        .send()
        .await?;

    // 6. Stream tokens one by one
    let stream = response.bytes_stream();
    // ... process stream ...
}
```

### IPC (Inter-Process Communication)

**What is IPC?** The communication bridge between React (frontend) and Rust (backend).

**How it works**:
1. Frontend calls `invoke('command_name', { params })`
2. Tauri serializes params to JSON
3. Tauri sends JSON to Rust backend
4. Rust backend receives JSON, deserializes
5. Rust executes command
6. Rust serializes result to JSON
7. Tauri sends JSON back to frontend
8. Frontend deserializes and uses result

**Example**:
```typescript
// Frontend (TypeScript)
const result = await invoke('get_models');

// Backend (Rust)
#[tauri::command]
fn get_models() -> Result<Vec<Model>, AppError> {
    // ... get models ...
    Ok(models)
}
```

---

## Language Guide: Rust

### What is Rust?

Rust is a systems programming language that focuses on:
- **Safety**: No memory bugs (no null pointers, no buffer overflows)
- **Performance**: Near C/C++ speed
- **Concurrency**: Safe parallel programming
- **Modern**: Great tooling and ecosystem

### Basic Rust Concepts

#### 1. Variables and Mutability

```rust
// Immutable by default
let x = 5;

// Mutable with 'mut'
let mut y = 10;
y = 20;

// Constants (SCREAMING_SNAKE_CASE)
const MAX_SIZE: usize = 100;
```

#### 2. Functions

```rust
// Function signature: fn name(params) -> return_type
fn add(a: i32, b: i32) -> i32 {
    a + b
}

// Async function (for network operations)
async fn fetch_data(url: &str) -> Result<String, AppError> {
    // ...
}
```

#### 3. Structs and Enums

```rust
// Struct (like a class but simpler)
struct Model {
    name: String,
    size: usize,
}

// Enum (like a union)
enum AppError {
    NetworkError(String),
    ValidationError(String),
    NotFound(String),
}

// Use with 'match'
match error {
    AppError::NetworkError(msg) => println!("Network error: {}", msg),
    AppError::ValidationError(msg) => println!("Validation error: {}", msg),
}
```

#### 4. Ownership and Borrowing

This is Rust's unique feature that prevents memory bugs:

```rust
// Ownership: value has one owner
let s1 = String::from("hello");
let s2 = s1; // s1 is moved, s2 now owns it

// Borrowing: read without taking ownership
fn print_length(s: &str) {
    println!("Length: {}", s.len());
}

let s = String::from("hello");
print_length(&s); // Borrow s, don't move it
```

#### 5. Error Handling

```rust
// Result<T, E> type
fn divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        Err(String::from("Cannot divide by zero"))
    } else {
        Ok(a / b)
    }
}

// Use 'match' or '?' operator
fn main() {
    let result = divide(10, 2);
    match result {
        Ok(value) => println!("Result: {}", value),
        Err(e) => println!("Error: {}", e),
    }
}
```

#### 6. Async/Await

```rust
use tokio::time::{sleep, Duration};

async fn fetch_data() -> Result<String, AppError> {
    sleep(Duration::from_secs(1)).await;
    Ok(String::from("data"))
}

async fn main() {
    let data = fetch_data().await?;
    println!("Got: {}", data);
}
```

### Rust in This Project

#### Cargo.toml — Dependencies

```toml
[dependencies]
tauri = "2"              # Desktop framework
serde = { version = "1", features = ["derive"] }  # Serialization
serde_json = "1"         # JSON handling
reqwest = "0.12"         # HTTP client
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

#### Main Entry Point

```rust
// backend/src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    splice_lib::run()
}
```

#### Tauri Command

```rust
// backend/src/commands/prompt.rs
use tauri::State;

#[tauri::command]
pub async fn run_prompt(
    model: String,
    prompt: String,
) -> Result<String, AppError> {
    // Validate input
    let prompt = validate_prompt(&prompt)?;

    // Run inference
    let output = InferenceBackend::run(&model, &prompt).await?;

    Ok(output)
}
```

---

## Language Guide: TypeScript

### What is TypeScript?

TypeScript is JavaScript with **type safety**. It adds static typing to catch errors before runtime.

### Basic TypeScript Concepts

#### 1. Types and Interfaces

```typescript
// Basic types
let name: string = "Alice";
let age: number = 30;
let isActive: boolean = true;
let id: string | number; // Union type

// Interface (like a class but for types)
interface Model {
  name: string;
  size: number;
}

// Type alias (for complex types)
type Prompt = {
  content: string;
  model: string;
  temperature: number;
};
```

#### 2. Functions

```typescript
// Typed function
function add(a: number, b: number): number {
  return a + b;
}

// Async function
async function fetchData(url: string): Promise<string> {
  const response = await fetch(url);
  return response.text();
}

// Optional parameters
function greet(name: string, greeting?: string): string {
  return greeting ? `${greeting}, ${name}!` : `Hello, ${name}!`;
}
```

#### 3. React Components

```typescript
// Functional component with TypeScript
interface Props {
  model: string;
  onRun: (model: string, prompt: string) => void;
}

function RunControls({ model, onRun }: Props) {
  const [prompt, setPrompt] = useState("");

  const handleRun = () => {
    onRun(model, prompt);
  };

  return (
    <button onClick={handleRun}>
      Run with {model}
    </button>
  );
}
```

#### 4. State Management (useState)

```typescript
import { useState } from 'react';

function App() {
  // useState returns [value, setter]
  const [count, setCount] = useState(0);
  const [name, setName] = useState<string>("");
  const [models, setModels] = useState<Model[]>([]);

  // Update state
  const increment = () => setCount(count + 1);
  const updateName = (e: React.ChangeEvent<HTMLInputElement>) => {
    setName(e.target.value);
  };

  return (
    <div>
      <p>Count: {count}</p>
      <button onClick={increment}>Increment</button>
    </div>
  );
}
```

#### 5. Custom Hooks

```typescript
// Custom hook for streaming
function useStreamingRun() {
  const [output, setOutput] = useState<string>("");
  const [status, setStatus] = useState<"idle" | "running" | "completed" | "error">("idle");
  const [error, setError] = useState<string | null>(null);
  const [metrics, setMetrics] = useState<Metrics | null>(null);

  const start = async (model: string, prompt: string) => {
    setStatus("running");
    setOutput("");
    setError(null);

    try {
      const result = await invoke('run_prompt', { model, prompt });

      // Handle streaming response
      for await (const chunk of result) {
        setOutput(prev => prev + chunk);
      }

      setStatus("completed");
    } catch (err) {
      setError(err as string);
      setStatus("error");
    }
  };

  const cancel = () => {
    setStatus("idle");
  };

  return { output, status, error, metrics, start, cancel };
}
```

#### 6. Zod Validation

```typescript
import { z } from 'zod';

// Define schema
const PromptSchema = z.object({
  content: z.string().min(1, "Prompt cannot be empty"),
  model: z.string().min(1, "Model is required"),
  temperature: z.number().min(0).max(2).optional(),
});

// Validate
function validatePrompt(data: unknown) {
  const result = PromptSchema.safeParse(data);

  if (!result.success) {
    throw new Error(result.error.errors[0].message);
  }

  return result.data;
}
```

### TypeScript in This Project

#### package.json — Dependencies

```json
{
  "dependencies": {
    "react": "^19.1.0",
    "react-dom": "^19.1.0",
    "@tauri-apps/api": "^2",
    "zustand": "^5.0.13",
    "zod": "^4.4.3",
    "@monaco-editor/react": "^4.7.0"
  },
  "devDependencies": {
    "typescript": "~5.8.3",
    "vite": "^7.0.4",
    "vitest": "^4.1.7",
    "@testing-library/react": "^16.3.2"
  }
}
```

#### Tauri Invoke

```typescript
// frontend/src/shared/ipc/client.ts
import { invoke } from '@tauri-apps/api/core';

// Call Rust command
async function getModels(): Promise<Model[]> {
  return await invoke('get_models');
}

async function runPrompt(model: string, prompt: string): Promise<string> {
  return await invoke('run_prompt', { model, prompt });
}
```

---

## Workflow Explained

### The Development Loop

For every unit of work (one step, one feature):

```
[1] Understand the step
    └─ Read the spec from docs/phase-roadmap.md
    └─ Write down expected input and output

[2] Implement the minimum
    └─ Smallest code change that satisfies the spec

[3] Write the test
    └─ One test per behavior

[4] Run the test
    └─ It must pass

[5] Verify the output (DATA QUALITY GATE)
    └─ Inspect actual output vs expected
    └─ Check shape, values, edge cases

[6] Update docs
    └─ Update doc if behavior changed

[7] Commit
    └─ Conventional Commits format

[8] Move on
    └─ Only now start next step
```

### Why This Loop?

1. **Test first** → You know what you're building
2. **Test pass** → Code works
3. **Verify output** → Code works correctly (not just the path you wrote)
4. **Update docs** → Knowledge is preserved
5. **Commit** → Changes are tracked
6. **Move on** → Don't pile up work

### Common Mistakes to Avoid

❌ **Stacking steps** — "Let me do steps 1-3 then test"
✅ **Do one step at a time**

❌ **Loosening assertions** — "Test fails, let me change assertion"
✅ **Fix the code** — Test is right

❌ **Skipping verification** — "Test passed, so it's good"
✅ **Verify output** — Tests verify the path, verification confirms it's the right path

❌ **Bundling docs** — "I'll update docs later"
✅ **Update docs now** — Later does not exist

---

## Development Setup

### Prerequisites

You need these installed on your computer:

1. **Rust** (for backend)
   ```bash
   brew install rust  # macOS
   # or
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh  # Linux
   ```

2. **Node.js** (for frontend)
   ```bash
   brew install node  # macOS
   # or
   nvm install 20  # Using Node Version Manager
   ```

3. **pnpm** (package manager)
   ```bash
   npm install -g pnpm  # Global install
   ```

4. **llama.cpp** (AI model runner)
   ```bash
   brew install llama_cpp  # macOS
   llama-server -m MODEL.gguf --port 8081 --jinja &  # Start llama.cpp server
   llama-server -m llama3.2:1b  # Download a model
   ```

### Installation Steps

1. **Clone the repository**
   ```bash
   git clone <repository-url>
   cd QM-Dev
   ```

2. **Install frontend dependencies**
   ```bash
   cd frontend
   pnpm install
   cd ..
   ```

3. **Install Rust dependencies**
   ```bash
   cd backend
   cargo build
   cd ..
   ```

4. **Initialize Git**
   ```bash
   git init
   git add .
   git commit -m "chore: initial setup"
   ```

5. **Start development server**
   ```bash
   pnpm tauri dev
   ```

### Running the App

```bash
# Development mode (with hot reload)
pnpm tauri dev

# Build for production
pnpm tauri build

# Run tests
pnpm test
```

### Common Commands

```bash
# Frontend
cd frontend
pnpm dev              # Start Vite dev server
pnpm build            # Build for production
pnpm test             # Run tests
pnpm test:watch       # Run tests in watch mode

# Backend
cd backend
cargo build           # Build Rust code
cargo test            # Run Rust tests
cargo clippy          # Run linter

# Full project
pnpm tauri dev         # Start Tauri app
pnpm tauri build       # Build desktop app
```

---

## Common Tasks

### Adding a New Feature

1. **Create feature folder**
   ```bash
   mkdir -p frontend/src/features/my-feature
   ```

2. **Create components**
   ```bash
   touch frontend/src/features/my-feature/components/MyComponent.tsx
   ```

3. **Create hooks**
   ```bash
   touch frontend/src/features/my-feature/hooks/useMyFeature.ts
   ```

4. **Create types**
   ```bash
   touch frontend/src/features/my-feature/types.ts
   ```

5. **Create tests**
   ```bash
   mkdir -p frontend/src/features/my-feature/__tests__
   touch frontend/src/features/my-feature/__tests__/MyComponent.test.tsx
   ```

6. **Add Rust command**
   ```bash
   touch backend/src/commands/my_feature.rs
   ```

7. **Register command in lib.rs**
   ```rust
   mod commands;
   use commands::my_feature;
   ```

### Debugging

**Frontend**:
```bash
cd frontend
pnpm dev
# Open browser DevTools (F12)
# Check Console for errors
# Check Network tab for API calls
```

**Backend**:
```bash
cd backend
cargo build
# Add println! for debugging
# Run with RUST_LOG=debug cargo run
```

**Tauri**:
```bash
# Check Tauri logs in terminal
# Check browser console for frontend errors
```

### Running Tests

```bash
# Frontend tests
cd frontend
pnpm test

# Backend tests
cd backend
cargo test

# All tests
pnpm test
cargo test
```

### Committing Changes

Use **Conventional Commits**:

```
feat: add new feature
fix: fix bug
docs: update documentation
test: add test
refactor: refactor code
chore: update dependencies
```

Examples:
```bash
git commit -m "feat: add model comparison feature"
git commit -m "fix: handle empty prompt error"
git commit -m "docs: update README with setup instructions"
git commit -m "test: add unit test for prompt validation"
```

### Code Style

**Rust**:
- Use `snake_case` for functions and variables
- Use `PascalCase` for types
- Use `SCREAMING_SNAKE` for constants
- No `unwrap()` outside tests
- Return `Result<T, AppError>` for errors

**TypeScript**:
- Use `camelCase` for functions and variables
- Use `PascalCase` for components and types
- Use `SCREAMING_SNAKE` for constants
- No `any` types (use `unknown` instead)
- Use interfaces for object shapes

### File Size Limit

**Hard limit: 100 lines per file**

If a file reaches 95 lines, split it now. Splits are by concern, not arbitrary halving.

---

## Learning Resources

### Rust
- [Rust Book](https://doc.rust-lang.org/book/) — Official Rust tutorial
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/) — Learn by examples
- [Rustlings](https://github.com/rust-lang/rustlings) — Interactive exercises

### TypeScript
- [TypeScript Handbook](https://www.typescriptlang.org/docs/) — Official docs
- [React TypeScript Cheatsheet](https://react-typescript-cheatsheet.netlify.app/) — React + TS tips

### Tauri
- [Tauri Documentation](https://tauri.app/v1/guides/) — Official Tauri guide
- [Tauri Examples](https://github.com/tauri-apps/tauri/tree/dev/examples) — Code examples

### React
- [React Documentation](https://react.dev/) — Official React docs
- [React Patterns](https://reactpatterns.com/) — Common React patterns

### General
- [MDN Web Docs](https://developer.mozilla.org/) — Web development reference
- [Stack Overflow](https://stackoverflow.com/) — Q&A for developers

---

## Summary

### Key Takeaways

1. **Splice** is a desktop app for running AI models locally
2. **Frontend**: React + TypeScript (web UI)
3. **Backend**: Rust (fast, safe, native code)
4. **Desktop**: Tauri (runs on all platforms)
5. **AI**: llama.cpp (runs models locally)

### Architecture
- Frontend talks to Backend via JSON IPC
- Backend talks to llama.cpp via HTTP
- Data flows: User → React → Tauri → Rust → llama.cpp → Rust → Tauri → React → User

### Workflow
1. Understand the step
2. Implement minimum code
3. Write test
4. Run test (must pass)
5. Verify output (data quality gate)
6. Update docs
7. Commit
8. Move on

### File Structure
- `frontend/` — React + TypeScript
- `backend/` — Rust
- `docs/` — Documentation
- `features/` — Feature modules
- `commands/` — Backend commands

### Language Rules
- **Rust**: `snake_case`, `Result<T, E>`, no `unwrap()`, async/await
- **TypeScript**: `camelCase`, interfaces, `useState`, `invoke()`
- **Files**: Max 100 lines, split by concern

### Development
- `pnpm tauri dev` — Start development
- `pnpm tauri build` — Build for production
- `pnpm test` — Run tests
- Use Conventional Commits
- Follow the workflow loop

---

## Next Steps

1. **Read the docs** — Start with `docs/setup.md` and `docs/workflow.md`
2. **Set up your environment** — Follow the setup instructions
3. **Run the app** — `pnpm tauri dev`
4. **Explore the code** — Read the files in `frontend/src/features/` and `backend/src/`
5. **Make a small change** — Try modifying a component or adding a feature
6. **Write a test** — Follow the workflow loop

Good luck! 🚀