# Developing meshcore-tui

How the UI is put together, and how to change it.

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
   `update` (`src/main.rs:78`). `Ui::handle_key` passes it to the tab on
   screen (`src/view.rs:62`). The Overview passes it to its focused panel, the
   input line (`src/overview.rs:43`).
2. **View → Action.** The input line takes its text and returns
   `KeyResult::Action(Action::Submit("hi"))` (`src/input.rs:95`). It can't send
   anything itself: views read `App` but never change it.
3. **Action → Effect.** `apply_action` hands the line to `App::submit`
   (`src/update.rs:75`). It isn't a slash command, so `submit` returns
   `Effect::Send { to: <the contact>, text: "hi" }` (`src/commands.rs:346`).
4. **Effect → radio.** The main loop sends the effect to the radio task
   (`src/main.rs:82`). `send_text` calls meshcore-rs and gets back
   `Outcome::DirectSent` (`src/radio.rs:199`). The UI keeps drawing while it
   waits.
5. **Result → App.** The outcome comes back as `Msg::EffectDone`.
   `apply_outcome` calls `on_direct_sent` (`src/commands.rs:333`), which adds
   the message to the log and the Activity panel (`src/app.rs:268`).
6. **App → screen.** The next `draw` (`src/ui.rs:21`) shows the sent message.

Every feature follows this path or part of it. A radio event starts at step 5
(`Msg::Radio` → `App::apply_radio_event`). A key that only moves a cursor stops
at step 2 with `KeyResult::Handled`.
