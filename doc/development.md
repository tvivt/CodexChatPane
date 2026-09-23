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

Run this from the repository root (network access is needed to install dependencies):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-app.ps1
```

The script runs `npm ci`, both frontend checks, `cargo fmt --check`, `cargo test --locked`, the frontend build, and the release Cargo build. CI runs the same script. CI uses Node.js 22 and Rust stable; using those versions locally aligns the environments more closely.

The final output is `release/CodexChatPane.exe`; Cargo's intermediate output stays in `src-tauri/target/release/`. The executable does not include a WebView2 Runtime installer, so the target machine needs WebView2 Runtime installed.

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
