# meshcore-tui

A ratatui chat client for MeshCore companion radios (crate `meshcore-chat`).
Toy project: commit on master.

## Build and test

- `cargo build`, `cargo test`, `cargo clippy --all-targets`. No new clippy warnings.
- Rust 1.88+ (`rust-version`), ratatui 0.30, crossterm 0.29.
- Snapshot tests use insta with out-of-band files in `src/snapshots/`. Review
  with `cargo insta review`, or accept with `INSTA_UPDATE=always cargo test`
  and read the snapshot diff before committing.
- Tests live in `#[cfg(test)] mod tests` beside the code. Shared fixtures are
  in `src/test_support.rs`.

## Architecture

Nested Elm architecture. `DEVELOPING.md` explains the UI patterns: the
message loop, its types, and one key press traced end to end. Read it before
changing UI code. Background and rejected alternatives:
`.agents/memories/ui-architecture.md` (parts of its migration plan are out of
date; the code wins).

- `App` (`app.rs`) is domain state, owned by the main loop. No `Arc<Mutex>`.
- `Ui` (`view.rs`) holds view state. Tabs and panels implement `View`;
  `handle_key` returns a `KeyResult`, which may carry an `Action` that asks for
  an `App` change. The focused panel gets keys first.
- `update.rs`: `update(app, ui, Msg) -> Vec<Effect>`. Synchronous, no I/O.
- `radio.rs` is the only code that talks to `MeshCore`. It runs `Effect`s in
  a background task and sends results back as `Msg::EffectDone`.
- `main.rs` is the runtime loop: drain radio events and effect results, poll
  stats every 5s, draw, wait up to 50ms for a key.
- No `_ =>` catch-alls in `Msg`/`Action` routing, so new variants fail to
  compile until handled.

## MeshCore protocol

- `meshcore-rs` is a path dependency on `../meshcore-rs`, a local checkout. The
  build uses whatever branch is checked out there.
- The firmware source in `../MeshCore` (upstream/main) is the authority on the
  companion protocol, mainly `examples/companion_radio/MyMesh.cpp`. Check
  meshcore-rs parsing against it before relying on it: it has decoded at
  least one push frame wrong.
- Changes to meshcore-rs or MeshCore are minimal, follow upstream style, and
  stay local. Never push or publish them.

## Live radio

The radio is production and shared with a public mesh.

- Never transmit (send messages, adverts, channel or contact changes) without
  explicit approval.
- The client may be running in a tmux pane. Read it with `tmux capture-pane`;
  don't send it keys.

## Issues

Tracked with git-bug, not in files.

- `git bug bug status:open label:bug` lists open bugs; `label:feature` for features.
- Label every issue `bug` or `feature`. Add `low-priority` only when asked.
- Prefix titles with the area when there is one: `[activity]`, `[contacts]`,
  `[overview]`. New tabs are titled `new tab: …`.
- Read `git bug <cmd> --help` before using an unfamiliar subcommand.

## Commits

- Subject: imperative, describes the user-visible effect ("Pad activity entry
  kinds so the hop column lines up"). Optional body says why.
- One change per commit; separate unrelated changes.
- `.agents/tasks.md` is working state. Never commit it.
