# Developing meshcore-tui

How to set up a checkout, how the UI is put together, and how to change it.

## Setup

[mise](https://mise.jdx.dev) installs the tools, and its `init` task sets up
the checkout:

```sh
mise install
mise run init
```

`init` installs the [hk](https://hk.jdx.dev) git hooks and points
`git blame` at `.git-blame-ignore-revs`, which lists bulk changes like
the `cargo fmt` reformat so blame skips them.

## The model, on one screen

Everything that happens becomes a `Msg`. `update` applies it to the state and
returns the `Effect`s it needs. Radio work runs in a background task, and its
result comes back as another `Msg`. The screen is redrawn from the state every
loop.

```
 key press ──────┐
 radio event ────┤
 stats timer ────┼──► Msg ──► update(app, ui, msg) ──► Vec<Effect>
 radio result ───┘                │                         │
        ▲                         │ changes                 │ radio work
        │                         ▼                         ▼
        │                    App + Ui                  radio.rs task ──► MeshCore
        │                         │                         │
        │                         ▼                         │
        │                  draw(app, ui)                    │
        └───────────────────────────────────────────────────┘
                         Msg::EffectDone(result)
```

### The names

| Name | What it is | Where |
|---|---|---|
| `App` | What we know about the radio and the mesh: contacts, channels, messages, activity, stats | `src/app.rs` |
| `Ui` | What's on screen: the tabs, which tab is showing, the theme | `src/view.rs` |
| `View` | A tab or panel. Draws itself, handles its keys, keeps its own cursor/scroll/focus | `src/view.rs` |
| `KeyResult` | What a view did with a key: `Ignored`, `Handled`, or `Action(..)` | `src/view.rs` |
| `Action` | A view asking for an `App` change, e.g. `Submit(line)`, `SelectContact(i)` | `src/update.rs` |
| `Msg` | Something that happened: a key, a radio event, a radio result, the stats timer | `src/update.rs` |
| `Effect` | Work `update` can't do itself: send a message, run a radio command, ring the bell | `src/update.rs` |
| `Command` | A parsed slash command | `src/commands.rs` |
| `Outcome` | What the radio returned for an `Effect` | `src/commands.rs` |

Three of these are easy to mix up:

- A **`Msg`** is something that *happened*. It goes into `update`.
- An **`Action`** is a view *asking* for an `App` change. Only views make them.
- An **`Effect`** is work *to do* outside the state. Only `update` makes them.

### One key press, end to end

Typing `hi` and pressing Enter on the Overview tab, with a contact selected:

1. **Key → view.** The main loop wraps the key in `Msg::Key` and calls
   `update`. `Ui::handle_key` passes it to the tab on screen. The Overview
   passes it to its focused panel, the input line.
2. **View → Action.** The input line's `handle_key` takes its text and
   returns `KeyResult::Action(Action::Submit("hi"))`. It can't send anything
   itself: views read `App` but never change it.
3. **Action → Effect.** `apply_action` hands the line to `App::submit`. It
   isn't a slash command, so `submit` returns
   `Effect::Send { to: <the contact>, text: "hi" }`.
4. **Effect → radio.** The main loop sends the effect to the radio task,
   which calls meshcore-rs and gets back `Outcome::DirectSent`. The UI keeps
   drawing while it waits.
5. **Result → App.** The outcome comes back as `Msg::EffectDone`.
   `App::apply_outcome` adds the message to the log and the Activity panel.
6. **App → screen.** The next `draw` shows the sent message.

Every feature follows this path or part of it. A radio event starts at step 5
(`Msg::Radio` → `App::apply_radio_event`). A key that only moves a cursor stops
at step 2 with `KeyResult::Handled`.

## Nested views

The Elm architecture is nested here. Each view's `handle_key` is that view's
own update: it changes the view's state (cursor, scroll, focus) and nothing
else. When the key should change the `App`, it returns an `Action`, and
`apply_action` makes the change. Views run inside `update`, so every change
still happens in one place, in order.

A tab can hold panels the same way. The Overview hands each key to its focused
panel, and takes the key itself only when the panel returns
`KeyResult::Ignored`.

### Rules

1. **Domain data lives on `App`, nowhere else.** Views keep only state that
   means nothing outside the screen: cursor, scroll, focus, open or closed.
   They read `&App` when they draw or take a key, and never keep a copy.
2. **Refer to things by identity, not position.** Use a contact's public key,
   not its index in a list. Lists reload and re-sort, and a stored position
   then points at something else.
3. **Only `radio.rs` talks to the radio.** If `update` needs the radio, it
   returns an `Effect`.
4. **No `_ =>` catch-alls in `Msg` or `Action` routing**, so a new variant
   fails to compile until it's handled.
5. **Key routing order**: the keys every tab shares (Ctrl-C, F-keys) come
   first, then the focused panel, then its tab. The first to use the key
   wins. Esc is the exception: it goes to the views first, so an open popover
   can close, and quits only when none of them uses it.

### Why not components or Flux

Both were considered and rejected.

- **Components** (ratatui's component template: `Box<dyn Component>`, each
  owning its data, actions broadcast to all). Our data is shared by every
  view. A new contact matters to the contacts panel, sender names, `/to`, the
  selection and the Contacts tab. Components that own data either keep copies
  that drift apart or share `Rc<RefCell<…>>`, which brings runtime borrow
  panics and no single ordered place where state changes. Our views are boxed
  too, but they own only screen state and read the one `App`.
- **Flux** (dispatcher, stores, change notifications). Notifications exist to
  limit re-renders, and ratatui redraws every frame anyway. Our changes cut
  across stores (a channel message touches messages, contacts and channels),
  so stores would have to wait on or read each other. Without those parts,
  Flux is Elm.

### Known pitfalls

- **Late or stale effect results.** A result can arrive after the user has
  moved on, or from before a reconnect. Tag requests whose timing matters
  with an id or generation, and drop stale ones.
- **Effects that must run in order**, like the startup sequence or a repeater
  login followed by a command. The radio task runs effects one at a time in
  the order they're sent; meshcore-rs serializes commands anyway, so effects
  keep the UI responsive rather than making radio calls parallel.
- **State only known while drawing.** Scroll offsets and layout areas come
  from the last frame. Keep them in view state, written by `draw`, in one
  clearly named place.
