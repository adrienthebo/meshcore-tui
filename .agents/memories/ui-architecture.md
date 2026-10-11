# UI architecture: The Elm Architecture (TEA)

Decided 2026-10-10. meshcore-tui uses The Elm Architecture, applied fully: one
model, messages, a synchronous `update` that returns effects, a `view` that
renders the model, and an effect executor that owns all radio I/O. It is not a
hybrid with the component or Flux patterns.

References:
- https://ratatui.rs/concepts/application-patterns/the-elm-architecture/
- https://ratatui.rs/concepts/application-patterns/component-architecture/
- https://ratatui.rs/concepts/application-patterns/flux-architecture/

## Shape

```
crossterm keys ─┐
radio events  ──┼─► Msg ─► update(&mut Model, Msg) -> Vec<Effect> ─► effect executor ─► MeshCore
tick          ──┤                    │                                     │
effect results ─┘◄───────────────────┼─────────────────────────────────────┘
                                     ▼
                           view(&Model, &mut Frame)
```

- **Model**: one struct owned by the main loop (no `Arc<Mutex>`), split into
  `domain` (contacts, channels, messages, packets, device info, radio stats,
  RX log) and `ui` (active screen, focus, popups, per-screen UI state).
- **Msg**: everything that can happen: `Key(KeyEvent)`, `Radio(RadioEvent)`,
  `Tick`, `EffectDone(result)`, plus one nested message type per screen.
- **update**: synchronous and free of I/O. It mutates the model and returns
  `Vec<Effect>`. Being a non-async fn means `.await` can't creep in.
- **Effect**: radio work described as data (`SendChannelMsg`, `GetContacts`,
  `PollRadioStats`, `Quit`, …). Elm's `Cmd`.
- **Effect executor** (`radio.rs`): the only code that touches `MeshCore`. It
  runs effects as tokio tasks and sends results back as `Msg::EffectDone`.
- **view**: renders the model. It takes `&mut` only for ratatui stateful-widget
  state (`ListState`, `TableState`) kept in `ui`.
- **Runtime** (`main.rs`): a thin `tokio::select!` loop over key events, radio
  events, ticks and effect results. It holds no logic.

## Rules

1. **Shared data lives in `domain`, nowhere else.** Reusable UI pieces own only
   state that means nothing outside the UI: cursor, scroll, selection, open or
   closed. Domain data is passed into `view` and `update` every time, never cached.
2. **Refer to selections by identity, not index.** Use a contact's key prefix,
   not its `Vec` position; indexes go stale when lists reload.
3. **All input goes through `update`.** UI pieces don't get their own
   `handle_key`. A reusable piece is a state struct, methods that change it, and
   a render function. `InputLine` is the model to copy: `App::handle_key` maps
   keys to `InputLine::insert` / `backspace` / … and the struct never sees a
   `KeyEvent`.
4. **Key routing order lives in one function**: modal or popup → focused piece →
   screen → global. The first to consume the key wins. Cover it with routing tests.
5. **Only the executor does radio I/O.** If `update` needs the radio, it returns
   an `Effect`.
6. **Every effect result is handled.** Results arrive as `EffectDone(Result<…>)`,
   and a default handler logs errors to the message panel unless a screen
   handles them.
7. **No `_ =>` catch-alls in message routing**, so the compiler flags any
   variant that isn't forwarded.
8. **Nest only when needed.** Add a screen sub-model (`State`, `Msg`, `update`,
   `view`) when a second screen exists, not before. Don't add a `Component`
   trait until there are 3 or more pieces that really share an interface.

## Known pitfalls and how we handle them

- **Late or stale effect results.** A result can arrive after the user has
  moved on, or from before a reconnect. Tag requests whose timing matters
  (polling, anything that can outlive a reconnect) with an id or generation, and
  drop stale results.
- **Effects that must run in order.** Examples: repeater login, then a CLI
  command; the startup sequence app start → set time → contacts → channels. Use
  a sequential effect. meshcore-rs serializes commands behind one mutex anyway,
  so effects make the UI responsive, not radio calls parallel.
- **Untested seams.** Pure `update` and `view` test easily; key routing and the
  executor don't. Test routing directly. Add a small radio trait for executor
  tests only when the executor exists.
- **Layout areas only known while rendering.** Mouse handling and scroll need
  the areas from the last frame. Store them in `ui`, written by `view` and read
  by `update`, in one clearly named place.
- **Large `update` and `Msg`.** Split them per screen as screens appear.

## Rejected alternatives

- **Component architecture** (ratatui component template: `Box<dyn Component>`
  each owning state, `Action` broadcast). Our domain data is shared by every
  view. A `NewContact` event matters to the contacts panel, chat sender
  resolution, `/to`, the selection, the RX log and the CLI. Components either
  keep copies that drift apart, can't borrow each other from the component
  `Vec`, or share `Rc<RefCell<…>>`, which means runtime borrow panics plus a
  single model without Elm's single ordered place where it changes. It fits apps
  made of independent panes with separate data sources, which this app isn't.
- **Flux** (dispatcher → stores → notify views). Change notification exists to
  limit re-renders in retained-mode UIs. ratatui redraws every frame, so it has
  nothing to do here. What's left is a forwarding dispatcher and multiple
  stores. Our updates cut across stores (a channel message touches messages,
  contacts and channels), which needs ordering between stores (`waitFor`) or
  stores reading each other. Stripped of the parts that don't apply, Flux is
  Elm.

## Migration (replaces Phase 8 of the test-coverage plan)

The test-coverage refactor already made the code update-shaped:
`App::handle_key`, the `App::on_*` radio event methods and
`App::apply_local` are update functions over one state.

1. **Runtime loop**: `tokio::select!` over crossterm `EventStream`, a
   radio-event channel and a tick. `App` is owned by the loop, and the shared
   mutex goes away.
2. **Effects**: radio commands and sending become `Effect`s.
   `handle_command` and `send_message` split into an `update` part and an
   executor part. This also stops the UI freezing during radio calls, and the
   screen being blank during `connect()`.
3. **Model split** into `domain` and `ui`. Change contact selection from an
   index to a key prefix.
4. **Screens**: add the screen layer when the second screen (diagnostics)
   arrives.

Each step keeps the existing tests passing and lands as its own commit.
