# Development and validation

[English](development.md) | [简体中文](development.zh-CN.md)

For the current feature map, handoff decisions, benchmark snapshot and limitations see [development reference](development.zh-CN.md) and [feature-to-code coverage](product.zh-CN.md#8-功能文档与代码对应).

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

Do not indiscriminately enable all ignored tests: some query live quotas or write app caches. Read-only `live_catalog_audit`, `live_analysis_audit` and `live_generation_replay` separately check metadata I/O, real-log equivalence/warm checkpoints, and forecast evidence. The latter two include local fixture IDs and need suitable local data. `CODEX_PANE_ROUND_VERIFY_IDS` selects round reconciliation IDs; `CODEX_PANE_VERIFY_METRICS_THREAD` selects latest/full comparison. To reuse the build script's cache, set `CARGO_HOME` to `.runtime/cargo-home` and `CARGO_TARGET_DIR` to `.runtime/app-target`, then use `--lib --locked --offline` where dependencies are cached.

## 3. Build the portable Windows executable

Run this from the repository root (npm packages must be available for the first install or dependency changes; Rust dependencies must already be cached):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-app.ps1
```

The local script uses content fingerprints to decide whether to run `npm ci` and the frontend build, reusing unchanged `node_modules` and `dist`. Missing dependencies or changed frontend artifacts also trigger rebuilding. Fingerprints live in `.runtime/build-cache/`. Cargo still checks for changes on every run, using `--release --locked --offline` and `CARGO_INCREMENTAL=1`. The first run after enabling incremental compilation establishes its cache; changed embedded resources or Rust code still require compilation/linking.

The final output is `.runtime/bin/CodexChatPane.exe`; Cargo artifacts stay in `.runtime/app-target/` and dependencies in `.runtime/cargo-home/`. Identical executables are not copied again; exit the running app before replacing a changed executable. The executable does not include a WebView2 Runtime installer, so the target machine needs WebView2 Runtime installed.

The development checkout's script does not run the full validation commands above; run the appropriate checks separately before release. In that checkout, run `powershell -NoProfile -File tests/build-app.ps1` for the isolated build-script regression check. It uses temporary files and fake npm/Cargo commands to verify cache invalidation and failure recovery without reinstalling project dependencies. The public release checkout retains its online build script, which runs validation and outputs `release/CodexChatPane-v<version>.exe` for GitHub Actions.

## 4. GitHub Actions

The development checkout has no publishing workflow. The public release checkout retains `Windows portable build`, triggered only by `v*` tags or a manual run. A tag matching the package version publishes `CodexChatPane-v<version>.exe`; a manual run only uploads the portable artifact. Maintain versioned notes under `doc/releases/`, then apply them to the GitHub Release after its CI build succeeds. A local build or backup commit is not a release. Feature/documentation updates do not automatically change versions, commit, push or publish.

The development repository is the source of truth for documentation. Its public `README.md`, `README.zh-CN.md`, `doc/`, and screenshots are copied to the release repository; internal design notes remain in the development repository. Update public documentation there once, then sync it for release.

Version fields are `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` (currently 0.5.0). Preserve the fixed local entry name CodexChatPane.exe. Generic output is `.runtime/bin/CodexChatPane.exe`; `D:\AI\CodexChatPane\CodexChatPane.exe` is this machine's deployment convention, not a general default. Before replacement, query and confirm the running executable path with Get-Process and CIM, exit that exact process, build, copy, and restart. Record new build/installed SHA256 equality, file time, new PID, path, StartTime and Responding. Equal hashes of two old binaries do not prove new code is deployed.

## 5. Change checklist

- Behavior changes: update `doc/product.md` and the corresponding frontend or Rust checks.
- Data source or snapshot changes: check `src-tauri/src/source*`, `src/app.js`, and test fixtures together.
- Deep link or MCP changes: update `doc/integration.md` and verify success, failure, and unavailable paths.
- Layout or interaction changes: update `doc/usage.md` and run frontend checks; use sanitized current data if visual review is needed.
- Settings changes: keep personal state under `%CODEX_HOME%\.codex-chat-pane\` out of commits.
- Keep measured single-request/turn summary, recentSpeed forecast samples and generation phases separate. Validate ownership, no inherited/child duplication, complete-range certificates, partial tails and old cumulative baselines when changing analytics. Preserve delayed changed IDs through view switches, discard old-account results, and stop queued work without consumers.
- Keep UI/menu, data contracts, integration/test events and documentation synchronized. The [metrics handoff](conversation-metrics.zh-CN.md), [consumption reference](usage-history.zh-CN.md), [menu matrix](usage.zh-CN.md#6-右键菜单速查) and [cache table](architecture.zh-CN.md#缓存与失效) identify the relevant interfaces and limits.
- Use the [full feature index](README.md) for documentation coverage. Update both language guides and affected numerical references; check relative links, anchors and superseded behavior. Documentation-only edits do not require a rebuild, restart or full local-log scan.

Historical evidence recorded on 2026-10-28: default Rust 101 passed/20 ignored, frontend and dynamic checks passed; three real logs reconciled usage and question rounds, and a latest request matched full details. Debug metadata reads took 34/25 ms for 578 chats with zero rollout bytes; deferred status was separately 3.8/2.1 seconds. Warm analysis read zero source content; cold analysis was not uniformly faster. Those real-data checks were not rerun for v0.5.0 and do not certify subsequent forecast changes. The [dated record](development.zh-CN.md#7-本次验证记录) contains measurements and reproducible entry points. No browser/native interaction acceptance, actual MCP writes, exhaustive account/file replacement combinations, full-history memory test or Release native startup benchmark was performed. Expanded compressed previews may still decode repeatedly; small append guards are not a full-file tampering proof.

For v0.5.0 on 2026-10-29, the synchronized public release checkout passed the default Rust suite (136 passed, 24 ignored), frontend checks, dynamic checks, Rust formatting, frontend production build, version/lockfile consistency, relative documentation file links, and `git diff --check`. The tagged workflow separately builds the downloadable executable. These automated checks do not replace native interaction acceptance.

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
