# Development and validation

[English](development.md) | [简体中文](development.zh-CN.md)

## 1. Development environment

- Windows
- Node.js/npm
- Rust stable / Cargo
- Codex Desktop (required to validate runtime data)

Install dependencies and start the development window:

```powershell
npm ci
npm run tauri dev
```

To preview only the frontend:

```powershell
npm run dev
```

## 2. Validation commands

Frontend build and framework-free checks:

```powershell
npm run build
node tests/frontend.mjs
node tests/dynamic.mjs
```

Rust formatting and tests:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

A few tests against local Codex data are ignored by default. They need real data or environment settings such as `CODEX_PANE_VERIFY_THREAD` and are not required for normal CI.

## 3. Build the portable Windows executable

Run this from the repository root (npm packages must be available for the first install or dependency changes; Rust dependencies must already be cached):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-app.ps1
```

The local script uses content fingerprints to decide whether to run `npm ci` and the frontend build, reusing unchanged `node_modules` and `dist`. Missing dependencies or changed frontend artifacts also trigger rebuilding. Fingerprints live in `.runtime/build-cache/`. Cargo still checks for changes on every run, using `--release --locked --offline` and `CARGO_INCREMENTAL=1`. The first run after enabling incremental compilation establishes its cache; changed embedded resources or Rust code still require compilation/linking.

The final output is `.runtime/bin/CodexChatPane.exe`; Cargo artifacts stay in `.runtime/app-target/` and dependencies in `.runtime/cargo-home/`. Identical executables are not copied again; exit the running app before replacing a changed executable. The executable does not include a WebView2 Runtime installer, so the target machine needs WebView2 Runtime installed.

The development checkout's script does not run the full validation commands above; run the appropriate checks separately before release. In that checkout, run `powershell -NoProfile -File tests/build-app.ps1` for the isolated build-script regression check. It uses temporary files and fake npm/Cargo commands to verify cache invalidation and failure recovery without reinstalling project dependencies. The public release checkout retains its online build script, which runs validation and outputs `release/CodexChatPane-v<version>.exe` for GitHub Actions.

## 4. GitHub Actions

The `Windows portable build` workflow runs the same script on `windows-latest` and uploads the executable as a `CodexChatPane-windows-portable` artifact. A `v*` tag push or a manual workflow run starts a build. A tag run also creates a GitHub Release after the build succeeds.

The development repository is the source of truth for documentation. Its public `README.md`, `README.zh-CN.md`, `doc/`, and screenshots are copied to the release repository; internal design notes remain in the development repository. Update public documentation there once, then sync it for release.

## 5. Change checklist

- Behavior changes: update `doc/product.md` and the corresponding frontend or Rust checks.
- Data source or snapshot changes: check `src-tauri/src/source*`, `src/app.js`, and test fixtures together.
- Deep link or MCP changes: update `doc/integration.md` and verify success, failure, and unavailable paths.
- Layout or interaction changes: update `doc/usage.md` and run frontend checks; use sanitized current data if visual review is needed.
- Settings changes: keep personal state under `%CODEX_HOME%\.codex-chat-pane\` out of commits.

## 6. Commit boundary

Commit only source, tests, lockfiles, necessary documentation, and the license to the public repository. Do not commit:

- `node_modules/`
- `dist/`
- `src-tauri/target/`
- `release/`
- `.runtime/`
- `.codegraph/`
- `src-tauri/gen/`
- Personal `settings.toml` and `folders.json` from `%CODEX_HOME%\.codex-chat-pane\`
- Local logs, conversation exports, or screenshots containing private data
