# How to Architect a Rust Codebase Like a Big-Tech Engineering Org

*A practical, opinionated reference for building large Rust systems that anyone on the team can navigate, test, and extend.*

---

## TL;DR — the one-paragraph answer

**No, idiomatic Rust is not MVC.** MVC came out of GUI and request/response web frameworks (Rails, Django, Spring MVC, Laravel) and is *controller-centric*. Rust's strengths — an exhaustive type system, ownership, and traits — push you toward **domain-centric** architecture instead. The pattern the serious Rust ecosystem has converged on is **Hexagonal Architecture (a.k.a. Ports & Adapters)**, which is the same core idea as **Clean Architecture** and **Onion Architecture**: isolate your business logic in a pure core, define the outside world as *traits* (ports), and implement those traits with swappable *adapters* (HTTP, DB, IPC, external APIs). At scale you express the layers physically as **Cargo workspace crates** so the compiler enforces the dependency direction for you. That's the whole game. Everything below is how to do it well.

You still have controllers, APIs, and DTOs — they just have different names and live in the outer ring. Translation table is in Part 1.

---

## Table of contents

1. [The MVC question, answered honestly](#part-1)
2. [The mental model: the Dependency Rule](#part-2)
3. [Physical structure: laying out the repo](#part-3)
4. [The layers in detail (with code)](#part-4)
5. [Error handling discipline](#part-5)
6. [Testing strategy](#part-6)
7. [Building & tooling](#part-7)
8. [Navigability conventions for huge codebases](#part-8)
9. [The playbook (copy-paste checklist)](#part-9)
10. [Appendix: crate cheat sheet](#appendix)

---

<a name="part-1"></a>
## Part 1 — The MVC question, answered honestly

### Why MVC doesn't transfer cleanly to Rust

MVC assumes a runtime where a "controller" object mediates between a "view" and a "model," usually wired together by reflection, inheritance, and a heavy framework. Rust gives you almost none of those mechanisms and doesn't want you to use them:

- **No inheritance.** MVC frameworks lean on base controllers and model superclasses. Rust replaces inheritance with composition + traits, which makes the *Ports & Adapters* style far more natural than MVC's class hierarchies.
- **Rust isn't web-first.** It's a systems language used for CLIs, services, desktop apps (Tauri), embedded, and game engines. MVC is a web/UI pattern. A pattern that only fits one delivery mechanism is a bad default for a language that targets many.
- **The type system wants to own your invariants.** Rust's killer feature is *making illegal states unrepresentable*. That pulls complexity into the domain model (entities, value objects, enums), which is exactly where Clean/Hexagonal puts it — and exactly where MVC tends *not* to.
- **Traits ARE ports.** A "port" in hexagonal architecture is an abstract interface the core depends on; an "adapter" implements it. That is *literally* what a Rust `trait` plus an `impl` block is. The pattern fits the language like a glove.

### What Rust uses instead

Three names, one idea. Pick whichever vocabulary your team likes — they describe the same concentric-rings structure:

| Name | Origin | Core idea |
|------|--------|-----------|
| **Hexagonal / Ports & Adapters** | Alistair Cockburn | Core logic talks to the world only through ports (traits); adapters implement them. **Most common name in the Rust world.** |
| **Clean Architecture** | Robert C. Martin | Concentric rings; dependencies point inward; framework/DB are outermost details. |
| **Onion Architecture** | Jeffrey Palermo | Domain at center, infrastructure at the edge. |

They all enforce the same **Dependency Rule** (Part 2). For Rust specifically, hexagonal + a bit of **Domain-Driven Design (DDD)** is the sweet spot, because DDD's "model the domain in its own language" maps onto Rust's expressive types beautifully.

### The translation table: your MVC vocabulary → Rust

This is the part you actually asked for. You don't lose controllers/APIs/DTOs — they move to the outer ring and get sharper names.

| MVC / web-framework term | Rust / hexagonal equivalent | Where it lives |
|--------------------------|------------------------------|----------------|
| Controller | **Handler** (axum/actix) or **Command** (`#[tauri::command]`) | Driving adapter (outer ring) |
| Route / endpoint definition | **Router** (`Router::new().route(...)`) | Driving adapter |
| Service (business logic) | **Use case** / application service | Application layer |
| Model (business) | **Entity** / **value object** / domain `enum` | Domain layer (center) |
| Model (persistence / ORM row) | **Repository impl** + a separate DB row struct | Driven adapter |
| Repository interface | **Port** = a `trait` | Domain or application layer |
| DTO (request/response) | **Request/Response struct** with `#[derive(Serialize, Deserialize)]` | Driving adapter |
| Dependency injection container | **Composition root** in `main.rs` — plain constructor calls, no magic | Entrypoint |
| Middleware / filters | **Tower layers** / Tauri middleware | Driving adapter |
| Exception | **Typed error enum** (`thiserror`) mapped to HTTP/IPC at the boundary | Crosses layers, mapped at edge |

**Key discipline:** the DTO is *not* the domain model. A `CreateUserRequest` (what the wire sends) is a different type from `User` (your domain entity). You map between them explicitly at the boundary. Conflating them is the #1 way "controllers" leak transport concerns into business logic.

---

<a name="part-2"></a>
## Part 2 — The mental model: the Dependency Rule

If you remember one sentence, remember this:

> **Source-code dependencies point only inward. The domain depends on nothing; everything depends on the domain.**

The rings, from center out:

```
        ┌─────────────────────────────────────────────────┐
        │  ENTRYPOINTS  (main.rs, Tauri setup, bin/)       │
        │  composition root: wires everything together     │
        │   ┌───────────────────────────────────────────┐  │
        │   │  ADAPTERS  (driving + driven)             │  │
        │   │  HTTP handlers / Tauri commands           │  │
        │   │  DB repos, Ollama HTTP client, filesystem │  │
        │   │   ┌───────────────────────────────────┐   │  │
        │   │   │  APPLICATION  (use cases)         │   │  │
        │   │   │  orchestrates domain + ports      │   │  │
        │   │   │   ┌───────────────────────────┐   │   │  │
        │   │   │   │  DOMAIN  (the core)        │   │   │  │
        │   │   │   │  entities, value objects,  │   │   │  │
        │   │   │   │  domain errors, PORTS      │   │   │  │
        │   │   │   │  (traits). Pure. No I/O.   │   │   │  │
        │   │   │   └───────────────────────────┘   │   │  │
        │   │   └───────────────────────────────────┘   │  │
        │   └───────────────────────────────────────────┘  │
        └─────────────────────────────────────────────────┘
            arrows of dependency  ───────────────►  point INWARD
```

**Driving adapters** drive the application (they call in): HTTP handlers, CLI parsers, Tauri commands, message-queue consumers.
**Driven adapters** are driven by the application (it calls out): database repos, the Ollama/llama.cpp HTTP client, the filesystem, the clock, RNG.

The application core never names a concrete adapter. It names a **port** (trait). The adapter implements the port. This is what makes the core testable (swap in a fake adapter) and the infrastructure replaceable (swap Ollama for MLX without touching business logic).

> **Litmus test:** can you `cargo build` your domain crate with the database client, the HTTP framework, and the IPC layer all deleted? If yes, your dependencies point the right way. If no, you have a leak.

---

<a name="part-3"></a>
## Part 3 — Physical structure: laying out the repo

You have two levers: **modules** (within a crate) and **crates** (within a workspace). The rule of thumb:

- **Small / early:** one crate, layers as modules. Don't over-engineer.
- **Real / growing / big-tech scale:** a **Cargo workspace** with one crate per layer (or per bounded context). Crate boundaries let the *compiler* enforce the Dependency Rule — `domain` literally cannot `use` `adapters` if it doesn't depend on it in `Cargo.toml`. This is the single biggest reason large Rust codebases stay navigable: the architecture is mechanically enforced, not just documented.

### 3a. Single-crate layout (start here)

```
src/
├── main.rs            # entrypoint + composition root (thin)
├── lib.rs             # re-exports, crate-level docs, prelude
├── domain/            # entities, value objects, domain errors, PORTS (traits)
│   ├── mod.rs
│   ├── model.rs
│   └── ports.rs
├── application/       # use cases; orchestrates domain via ports
│   └── mod.rs
├── adapters/
│   ├── inbound/       # driving: http handlers / cli / tauri commands
│   └── outbound/      # driven: db repos, external clients, fs, clock
└── config.rs
tests/                 # black-box integration tests
```

Dependency flow: `adapters → application → domain`. `main.rs` is allowed to see everything (it's the composition root).

### 3b. Workspace layout (the answer for a large codebase)

This is what scales to hundreds of thousands of lines and dozens of engineers. Each crate is a wall the compiler won't let you climb in the wrong direction.

```
my-system/
├── Cargo.toml                # [workspace] members + shared [workspace.dependencies]
├── crates/
│   ├── domain/               # PURE. zero infra deps. entities + ports (traits).
│   │   ├── Cargo.toml        # deps: almost nothing (maybe thiserror, uuid, time)
│   │   └── src/lib.rs
│   ├── application/          # use cases. depends on: domain
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── adapters/             # impls of ports. depends on: domain (+ application)
│   │   ├── Cargo.toml        # deps: sqlx/reqwest/etc live HERE, nowhere inward
│   │   └── src/lib.rs
│   ├── http/                 # (web case) axum router + handlers + DTOs
│   │   └── src/lib.rs
│   └── fixtures/             # shared test data + fakes
│       └── src/lib.rs
└── bin/
    └── server/               # the actual binary: composition root only
        ├── Cargo.toml        # depends on EVERYTHING; wires it together
        └── src/main.rs
```

The `banker` example pattern that's circulating in the Rust community names them `*-core`, `*-adapters`, `*-fixtures`, `*-http` — same shape, different prefix. Use whatever naming your org standardizes on; just keep it consistent.

**`Cargo.toml` workspace root** (centralize versions so every crate agrees):

```toml
[workspace]
resolver = "2"
members = ["crates/*", "bin/*"]

[workspace.dependencies]
# declare once, reference with `tokio.workspace = true` in member crates
tokio   = { version = "1", features = ["full"] }
serde   = { version = "1", features = ["derive"] }
thiserror = "2"
anyhow  = "1"
tracing = "0.1"
```

### 3c. The Tauri desktop-app layout (your QuantaMind case)

A Tauri app is *still* hexagonal — the only thing that changes is the driving adapter. Instead of HTTP handlers, your inbound edge is `#[tauri::command]` functions that the React frontend invokes over IPC. Everything inside the hexagon is identical.

```
src-tauri/
├── Cargo.toml                # ideally itself a workspace
├── crates/
│   ├── engine-core/          # DOMAIN: benchmark model, scoring, pass^k,
│   │   │                     #   verdict types, ResponderKind enum, PORTS.
│   │   └── src/lib.rs        #   PURE. no HTTP, no Tauri, no fs.
│   ├── engine-app/           # APPLICATION: "run readiness gate" use case,
│   │   └── src/lib.rs        #   orchestrates scoring over a Backend port.
│   └── backends/             # DRIVEN ADAPTERS: Ollama / llama.cpp / MLX
│       └── src/lib.rs        #   HTTP clients implementing the Backend trait.
└── src/
    ├── main.rs               # composition root: build adapters, inject, run app
    └── commands.rs           # DRIVING ADAPTER: #[tauri::command] fns =
                              #   thin wrappers that call use cases + map errors
frontend/                     # React (the "view" — but it's a real client app,
                              #   not an MVC view; it owns its own state)
```

Why this matters specifically for a determinism-critical engine: keeping `engine-core` pure means your scoring logic is a **pure function of (immutable inputs, calls)** with no hidden I/O — which is exactly what makes it reproducibly testable with property tests and snapshots (Part 6). The HTTP-to-Ollama concern lives in `backends/`, behind a `Backend` trait, so a test can inject a recorded-trace fake and never touch the network.

---

<a name="part-4"></a>
## Part 4 — The layers in detail (with code)

### 4a. Domain layer — make illegal states unrepresentable

The domain is the part competitors can't copy and the part that must never depend on a framework. Three tools:

**1. Newtypes / value objects** — wrap primitives so invariants are enforced at construction and can't be bypassed:

```rust
// domain/src/model.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email(String);

impl Email {
    pub fn parse(raw: String) -> Result<Self, DomainError> {
        if raw.contains('@') {
            Ok(Self(raw))
        } else {
            Err(DomainError::InvalidEmail)
        }
    }
    pub fn as_str(&self) -> &str { &self.0 }
}
```

Once you have an `Email`, it is *always* valid. No defensive checks scattered across the codebase. This is the Rust analogue of a "rich domain model," and it's strictly better than MVC's anemic data-bag models.

**2. Enums for closed sets** — when something has a fixed set of variants, an `enum` gives you compile-time exhaustiveness. The compiler forces every `match` to handle every case, so adding a variant surfaces every site that must change. (This is exactly why keeping a determinism-critical discriminator like `ResponderKind` an `enum` rather than a string is correct: you want the compiler screaming at you on every seam when the set changes, not a silent default branch.)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict { Ready, Conditional, NotReady }
```

**3. Domain errors** as a typed enum — the *complete* description of what can go wrong in a domain operation:

```rust
#[derive(Debug, thiserror::Error)]
pub enum CreateUserError {
    #[error("a user with email {0} already exists")]
    DuplicateEmail(String),
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),  // catch-all for unexpected infra failures
}
```

Now every caller of this operation gets a compiler-checked list of every failure it must handle. That's a level of safety MVC exception handling can't give you.

### 4b. Ports — the traits the core depends on

A port is the interface the application needs from the outside world, *expressed in the domain's language*, not the infrastructure's. Note it returns domain types and domain errors, and says nothing about SQL or HTTP:

```rust
// domain/src/ports.rs   (or application layer — either is defensible)
use async_trait::async_trait;   // until native async-in-traits covers your case

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn save(&self, user: &User) -> Result<(), CreateUserError>;
    async fn find_by_email(&self, email: &Email) -> Result<Option<User>, anyhow::Error>;
}
```

For QuantaMind, the equivalent is a `Backend` port:

```rust
#[async_trait]
pub trait Backend: Send + Sync {
    /// Issue one model call. The engine core never knows if this is Ollama,
    /// llama.cpp, MLX, or a recorded-trace fake used in tests.
    async fn invoke(&self, req: &ModelCall) -> Result<ModelResponse, BackendError>;
}
```

### 4c. Application layer — use cases

A use case orchestrates domain objects through ports. It is *thin* — it contains the workflow, not the rules (rules live in the domain) and not the I/O (that's in adapters). It depends on the **trait**, never a concrete type:

```rust
// application/src/create_user.rs
pub struct CreateUser<R: UserRepository> {
    repo: R,
}

impl<R: UserRepository> CreateUser<R> {
    pub fn new(repo: R) -> Self { Self { repo } }

    pub async fn execute(&self, raw_email: String) -> Result<User, CreateUserError> {
        let email = Email::parse(raw_email)
            .map_err(|_| CreateUserError::Unknown(anyhow::anyhow!("bad email")))?;

        if self.repo.find_by_email(&email).await?.is_some() {
            return Err(CreateUserError::DuplicateEmail(email.as_str().into()));
        }
        let user = User::new(email);
        self.repo.save(&user).await?;
        Ok(user)
    }
}
```

Generic `<R: UserRepository>` gives you **static dispatch + zero-cost injection**; in tests you instantiate `CreateUser::new(InMemoryUserRepo::default())`. (Prefer `Arc<dyn Trait>` when you need runtime polymorphism or to avoid generic bloat — both are idiomatic; pick per case.)

### 4d. Driving adapters — handlers / commands / DTOs

This is your "controller" layer. Its only jobs: deserialize the request DTO, call the use case, serialize the response DTO, map errors to a transport status. **No business logic here.**

**Web (axum) handler:**

```rust
// http/src/users.rs
#[derive(serde::Deserialize)]
struct CreateUserRequest { email: String }      // DTO in

#[derive(serde::Serialize)]
struct UserResponse { id: String, email: String } // DTO out

async fn create_user_handler(
    State(uc): State<Arc<CreateUser<PgUserRepo>>>,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = uc.execute(body.email).await?;     // delegate, don't think
    Ok(Json(UserResponse { id: user.id().to_string(), email: user.email().as_str().into() }))
}
```

**Tauri command (your case)** — structurally identical, IPC instead of HTTP:

```rust
// src/commands.rs
#[tauri::command]
async fn run_readiness_gate(
    state: tauri::State<'_, AppState>,
    req: GateRequest,                 // DTO from the React frontend
) -> Result<GateReport, ApiError> {   // DTO back to the frontend
    state.run_gate.execute(req.into_domain()).await
        .map(GateReport::from_domain)
        .map_err(ApiError::from)
}
```

**The DTO mapping discipline** — keep `From`/`TryFrom` impls at the boundary so the mapping is explicit and one place:

```rust
impl GateReport {
    fn from_domain(v: domain::GateResult) -> Self { /* ... */ }
}
impl GateRequest {
    fn into_domain(self) -> domain::GateSpec { /* ... */ }
}
```

> A trap worth flagging because it bites teams using typed frontends: if your frontend validates DTOs with something like Zod, remember a `z.object()` silently *strips* fields it doesn't list. Every new field on a domain spec must be added to the schema/registry too, or it vanishes on the round-trip with no error. Keep DTO field lists and your shared type registry in lockstep, and add a test that fails when they drift.

### 4e. Composition root — the only place that knows everything

`main.rs` builds the concrete adapters and injects them. This is your DI container, except it's just… constructor calls. No reflection, no framework magic, fully compiler-checked.

```rust
// bin/server/src/main.rs
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let pool   = PgPool::connect(&config.database_url).await?;

    // wire driven adapters
    let user_repo = PgUserRepo::new(pool);

    // wire use cases
    let create_user = Arc::new(CreateUser::new(user_repo));

    // wire driving adapter + run
    let app = http::router(create_user);
    axum::serve(listener, app).await?;
    Ok(())
}
```

Everything above `main` is decoupled; `main` is the one allowed coupling point. This is why the architecture stays clean: there is exactly one file where the wiring lives, and it's obvious.

---

<a name="part-5"></a>
## Part 5 — Error handling discipline

The community consensus is settled and worth following to the letter:

- **`thiserror` in libraries / the domain.** Define explicit, typed error enums. Callers get exhaustive, compiler-checked handling. Each domain operation gets its own error type that fully describes its failure modes.
- **`anyhow` in binaries / the composition root / top-level handlers.** Where you just need "something went wrong, here's a backtrace and context," `anyhow::Result` with `.context("...")` is ergonomic and subject to little churn.
- **Map at the boundary.** Domain errors are *domain* concepts. The adapter translates them into transport concepts (HTTP 409, IPC error payload) — e.g. `impl From<CreateUserError> for ApiError`. The domain never imports `StatusCode`.
- **Use a catch-all `Unknown(#[from] anyhow::Error)` variant** in domain errors so unexpected infrastructure failures can propagate without forcing you to enumerate every possible cause of, say, a dropped DB connection.

**Never fabricate a value to make an error go away.** If a computation can't produce a real result, the type should say so — `Option`, `Result`, or an explicit `N/A`/`Unknown` variant — not a placeholder `0` or empty string that downstream code will silently treat as real data. A fabricated `0` is a lie the type system would have caught if you'd let it. Make the absence representable and propagate it.

---

<a name="part-6"></a>
## Part 6 — Testing strategy

Rust nudges you toward testing (the type system removes a whole class of tests you'd write in dynamic languages), but the compiler can't catch *wrong behavior*. The mature 2025-era Rust stack uses several test types together — they answer different questions. **Test behavior and properties, not mechanisms:** assert *what* the code guarantees, never *how* it currently does it, or every refactor breaks green tests for no reason.

### The Rust test pyramid

```
                  ╱╲     e2e / integration (tests/ dir, real-ish adapters)
                 ╱  ╲    ── verifies wiring & boundaries; fewer, slower
                ╱────╲
               ╱      ╲  property + snapshot (proptest, insta)
              ╱        ╲ ── catch edge cases & output regressions
             ╱──────────╲
            ╱            ╲ unit tests (#[cfg(test)] inline)
           ╱______________╲ ── fast, many; test domain logic in isolation
```

### 1. Unit tests — inline, with access to privates

The idiomatic Rust pattern: a `#[cfg(test)] mod tests` submodule *inside the same file*. It compiles only under `cargo test`, keeps the release binary slim, and (uniquely) can see private items.

```rust
pub fn pass_k(passes: u32, k: u32) -> f64 { /* ... */ }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pass_k_is_one_when_all_pass() {
        assert_eq!(pass_k(5, 5), 1.0);
    }
}
```

### 2. Integration tests — black-box, in `tests/`

Files in the top-level `tests/` directory are each compiled as a separate crate that links your library as an external user would. They can only touch your **public** API — which is exactly what you want for testing module boundaries and wiring. Put shared helpers in `tests/common/mod.rs`.

```
tests/
├── common/mod.rs       # shared fixtures (note: mod.rs avoids it being its own test bin)
└── gate_flow.rs        # exercises the real use case against a fake backend
```

### 3. Trait-based mocking — `mockall`

Because your ports are traits, mocking is trivial and *honest* (you mock the interface you actually depend on, not a concrete class). Annotate the trait and `mockall` generates a `MockUserRepository` you can set expectations on:

```rust
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait UserRepository: Send + Sync { /* ... */ }

#[tokio::test]
async fn rejects_duplicate_email() {
    let mut repo = MockUserRepository::new();
    repo.expect_find_by_email()
        .returning(|_| Ok(Some(User::sample())));
    let uc = CreateUser::new(repo);
    assert!(matches!(uc.execute("a@b.com".into()).await,
                     Err(CreateUserError::DuplicateEmail(_))));
}
```

Often a hand-written `InMemoryRepo` fake is even better than a mock for use-case tests — it tests behavior (state in, state out) rather than call sequences (mechanism). Use `mockall` when you specifically need to assert *how* a port was called.

### 4. Property-based testing — `proptest`

Instead of hand-picking examples, you declare a *property* that must hold for all inputs; `proptest` generates hundreds of random cases and, on failure, **shrinks** the input to the minimal failing example. This is gold for anything with mathematical or determinism guarantees (scoring, parsing, serialization round-trips):

```rust
proptest! {
    #[test]
    fn pass_k_is_bounded(passes in 0u32..=100, k in 1u32..=100) {
        let p = pass_k(passes.min(k), k);
        prop_assert!((0.0..=1.0).contains(&p));
    }

    #[test]
    fn spec_roundtrips_through_json(spec in any::<AgenticSpec>()) {
        let json = serde_json::to_string(&spec).unwrap();
        let back: AgenticSpec = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(spec, back);   // catches "field silently dropped" bugs
    }
}
```

A round-trip property like the second one is precisely the test that catches a schema-strips-unlisted-fields bug deterministically, instead of hoping a hand-written example happened to include the dropped field.

### 5. Snapshot testing — `insta`

For complex structured output (a rendered report, a serialized env view, a verdict object), `insta` records the output once, then fails if it ever changes. `cargo insta review` lets you accept intended changes interactively. **Review snapshot diffs carefully** — blindly accepting them turns a regression detector into a rubber stamp.

```rust
#[test]
fn gate_report_shape_is_stable() {
    let report = run_gate(sample_spec());
    insta::assert_yaml_snapshot!(report);
}
```

Caveat for anything non-deterministic or model-produced (e.g. a replay/`EnvView` that's a function of live calls): snapshot the *structure and invariants*, not byte-for-byte model text, or your snapshots will be flaky. Snapshot the deterministic projection, not the stochastic source.

### 6. Fixtures & parameterized cases — `rstest`

`rstest` gives you injectable fixtures and table-driven `#[case(...)]` tests, cutting boilerplate:

```rust
#[rstest]
#[case(4, 4, 1.0)]
#[case(0, 4, 0.0)]
fn pass_k_cases(#[case] p: u32, #[case] k: u32, #[case] expected: f64) {
    assert_eq!(pass_k(p, k), expected);
}
```

### 7. Doctests — examples that can't rot

Code in `///` doc comments is compiled and run by `cargo test`. Your documentation literally cannot go stale, because CI fails if the example stops compiling. Use them for the "how do I call this" snippet on every public API.

### 8. Run it all fast — `cargo nextest`

`cargo-nextest` is the modern test runner: faster (better parallelism), cleaner output, per-test timeouts, and good CI ergonomics. Adopt it once your suite grows past a few seconds.

### Testing principles to hang on the wall

- **Don't chase 100% coverage; cover 100% of critical paths and meaningful scenarios.**
- **Green tests don't catch what only live traces catch.** Mocks verify *your* assumptions about a dependency; they say nothing about whether the real Ollama/DB behaves that way. Keep a thin layer of integration tests against the real thing (or recorded real traces) for the seams that matter.
- **Verify properties, not mechanisms.** A test that asserts the internal sequence of calls breaks on every refactor and protects nothing. A test that asserts the externally observable guarantee survives refactors and protects the contract.
- **Stamp the mode under test.** If a result can be produced two ways (e.g. native tool-calling vs. a fallback path), record which mode produced it and assert against the right one — never conflate them or pick "best of."
- **Beware the stale-binary trap.** If you compile fixtures/scenarios *into* the binary (`include_str!`), a stale build silently tests old data. Rebuild from HEAD before trusting a run, and make CI do it from clean.

---

<a name="part-7"></a>
## Part 7 — Building & tooling

### Cargo workspace mechanics

- **One lockfile, shared target dir, shared dependency versions.** Declare common deps in `[workspace.dependencies]` and reference them with `dep.workspace = true`. This guarantees every crate agrees on versions and dramatically improves incremental build times.
- **Features for optional capability, not for code paths.** Use Cargo features to toggle real optional functionality (`postgres` vs `sqlite`, `metrics`). Don't use them as a poor man's config flag for runtime behavior.
- **Profiles.** Tune `[profile.release]` (`lto = "thin"`, `codegen-units = 1` for max runtime perf; `opt-level = "z"` if binary size matters — relevant for a shipped desktop app). Keep a fast `[profile.dev]`.

### The lint gate (make CI enforce it)

Run these on every PR; a red lint = a red build:

```bash
cargo fmt --all -- --check          # formatting is not a code-review topic
cargo clippy --all-targets --all-features -- -D warnings   # warnings are errors
cargo test --workspace              # or: cargo nextest run --workspace
cargo doc --no-deps --workspace     # doctests + broken-link detection
```

Add `#![deny(unsafe_code)]` at the top of crates that have no business using `unsafe` (your domain crate definitely qualifies). Consider `cargo-deny` for license/advisory/duplicate-dependency auditing, and `cargo-audit` for the RustSec vulnerability DB — both standard in security-conscious orgs.

### CI skeleton (GitHub Actions shape)

```yaml
jobs:
  ci:
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2      # cache target/ — huge speedup
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets --all-features -- -D warnings
      - run: cargo nextest run --workspace
      - run: cargo doc --no-deps --workspace
```

Cache the `target/` directory and registry; it's the difference between a 2-minute and a 20-minute pipeline. Use a build matrix if you ship cross-platform (very relevant for a Tauri app: macOS/Windows/Linux).

---

<a name="part-8"></a>
## Part 8 — Navigability conventions for a huge codebase

Architecture keeps the *dependencies* clean. These conventions keep the *code* findable — the thing that actually determines whether a new engineer is productive in week one or week six.

### 1. The facade / re-export pattern

A crate's internal module tree is an implementation detail. Expose a curated public surface from `lib.rs` with `pub use`, and keep the rest private. Now you can refactor the internal layout without breaking any consumer, and readers see a clean menu instead of a maze:

```rust
// crates/adapters/src/lib.rs
mod ollama;
mod llamacpp;
mod helpers;          // stays private

pub use ollama::OllamaBackend;
pub use llamacpp::LlamaCppBackend;
// helpers is invisible to the outside world
```

### 2. The prelude pattern

For a crate with a handful of types everyone imports together, provide a `prelude` module so callers write one `use`:

```rust
pub mod prelude {
    pub use crate::{Backend, Verdict, GateReport, BackendError};
}
// downstream: use engine_core::prelude::*;
```

### 3. Crate-level docs (`//!`) as the front door

The top of every `lib.rs` should explain, in `//!` doc comments: what the crate is for, the quick-start snippet, and a one-line map of its modules ("Domain in [`model`], ports in [`ports`], …"). `cargo doc` turns this into the landing page. This is the highest-leverage documentation you can write.

### 4. Naming & file conventions (pick once, enforce always)

- Module/file names are `snake_case` nouns describing *what's inside* (`scoring.rs`, not `utils.rs` — "utils" is where findability goes to die).
- One concept per file; split when a file passes ~300–500 lines.
- Keep the layer obvious from the path: anyone should infer `crates/adapters/src/ollama.rs` is a driven adapter without opening it.
- Tests next to code (`#[cfg(test)]`) for units; `tests/` for integration. Consistency means a reader always knows where to look.

### 5. Architecture Decision Records (ADRs)

Keep a `docs/adr/` folder of short, dated, append-only notes capturing *why* a non-obvious decision was made (e.g. "why `ResponderKind` is an enum and not a string," "why answer-key changes mint a new versioned collection instead of editing in place"). When someone asks "why is this weird?" six months later, the answer is in the repo, not in someone's memory. This is the single cheapest thing big orgs do to stay sane and the first thing missing from codebases that rot.

### 6. Enforce the dependency direction mechanically

The workspace already prevents inward-pointing crates from depending outward. Add `cargo-deny`'s `[bans]` or a small CI check to forbid specific cross-layer dependencies you never want to appear. The goal: a wrong-direction `use` should fail CI, not survive to code review.

### 7. A top-level `ARCHITECTURE.md`

One file at the repo root that explains the crate graph and the Dependency Rule in five minutes. Matthew Brandaleone / matklad's "ARCHITECTURE.md" convention is widely adopted in big Rust projects precisely because it gives newcomers the map before they get lost in the territory. Link it from the README.

---

<a name="part-9"></a>
## Part 9 — The playbook (copy-paste checklist)

**Architecture**
- [ ] Dependencies point inward only. Domain depends on nothing.
- [ ] Domain crate compiles with all infra crates deleted (the litmus test).
- [ ] Every external dependency (DB, HTTP, IPC, clock, RNG) sits behind a port (trait).
- [ ] One composition root (`main.rs`) does all wiring; nothing else does DI.
- [ ] DTOs ≠ domain models; mapping is explicit at the boundary.

**Types & errors**
- [ ] Invariants enforced by newtypes/value objects, not scattered validation.
- [ ] Closed sets are `enum`s for compile-time exhaustiveness.
- [ ] `thiserror` in libs/domain; `anyhow` in bins/edges; map errors at the boundary.
- [ ] Absence is representable (`Option`/`N/A`/`Unknown`) — never a fabricated `0`.

**Testing**
- [ ] Unit tests inline (`#[cfg(test)]`); integration tests in `tests/`.
- [ ] Ports mocked via `mockall` or hand-written in-memory fakes.
- [ ] Property tests (`proptest`) for math/determinism/round-trips.
- [ ] Snapshot tests (`insta`) for complex *deterministic* output only.
- [ ] At least one real-trace integration test per critical seam (mocks lie).
- [ ] Tests assert observable behavior/properties, never internal mechanism.
- [ ] Stamp which mode/path produced a result; rebuild from HEAD before trusting runs.

**Build & navigability**
- [ ] Workspace with `[workspace.dependencies]`; one crate per layer.
- [ ] CI gate: `fmt --check`, `clippy -D warnings`, `nextest`, `cargo doc`.
- [ ] `#![deny(unsafe_code)]` on crates that shouldn't use it.
- [ ] Curated public surface via `pub use`; internals private.
- [ ] Crate-level `//!` docs + `ARCHITECTURE.md` + `docs/adr/`.
- [ ] No `utils.rs`. Files named for what's inside.

---

<a name="appendix"></a>
## Appendix — crate cheat sheet (current, by job)

| Job | Go-to crate(s) | Notes |
|-----|----------------|-------|
| Async runtime | `tokio` | The default. |
| HTTP server (web) | `axum` (on `tower`/`hyper`) | Composable, tower-based middleware. `actix-web` is the high-perf alternative. |
| Desktop shell | `tauri` v2 | Commands are your driving adapter. |
| HTTP client (driven) | `reqwest` | For calling Ollama/llama.cpp/MLX endpoints. |
| Serialization / DTOs | `serde` + `serde_json` | `#[derive(Serialize, Deserialize)]`. |
| Errors (libs) | `thiserror` (v2) | Typed error enums. |
| Errors (bins/edges) | `anyhow` | Context + backtraces. |
| SQL | `sqlx` (compile-checked) / `sea-orm` / `diesel` | Behind a repository port, always. |
| Logging/tracing | `tracing` + `tracing-subscriber` | Structured, async-aware observability. |
| Trait async (until fully stable) | `async-trait` | Needed for `dyn`-compatible async ports. |
| Mocking | `mockall` | Generates mocks from traits. |
| Property tests | `proptest` | Generation + shrinking. |
| Snapshot tests | `insta` | `cargo insta review`. |
| Fixtures / param tests | `rstest` | Injectable fixtures, `#[case]`. |
| Benchmarks | `criterion` | Statistics-driven microbenchmarks. |
| Test runner | `cargo-nextest` | Faster, cleaner than `cargo test`. |
| Supply-chain / licenses | `cargo-deny`, `cargo-audit` | License + RustSec advisory gates. |

---

### Who actually builds Rust this way

The pattern isn't academic — it's what shipping systems at scale use. AWS builds Firecracker (the microVM behind Lambda/Fargate) and Bottlerocket in Rust; Cloudflare replaced its nginx proxy fleet with Pingora; Discord rewrote latency-critical services from Go to Rust; Dropbox's file-sync engine is Rust; Meta ships the Buck2 build system in Rust; Microsoft uses it in Azure and Windows components. None of them organize Rust as MVC. They organize it as layered/hexagonal crate workspaces with the core logic isolated from infrastructure — exactly the structure above — because that's what keeps a multi-hundred-thousand-line, many-team codebase buildable, testable, and navigable.

*Start simple (single crate, modules as layers). Evolve to a workspace when team size or build times demand it. Let the compiler enforce the boundaries, and write the map (`ARCHITECTURE.md` + ADRs) so the next person doesn't have to reverse-engineer it.*
