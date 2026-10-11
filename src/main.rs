mod app;
mod commands;
mod input;
mod radio;
mod stats;
#[cfg(test)]
mod test_support;
mod text;
mod theme;
mod ui;
mod update;
mod view;

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event as CEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc::{self, UnboundedSender};

use app::App;
use radio::{connect, run_radio_effects, setup_subscriptions};
use theme::ThemeName;
use ui::draw;
use update::{update, Effect, Msg};

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "meshcore-chat", about = "TUI chat for MeshCore mesh networking")]
struct Args {
    /// Serial port (e.g. /dev/ttyACM0)
    #[arg(short, long)]
    port: Option<String>,

    /// TCP host:port
    #[arg(short, long)]
    tcp: Option<String>,

    /// BLE device name (scans for MeshCore devices if omitted)
    #[arg(long)]
    ble: Option<Option<String>>,

    /// Baud rate
    #[arg(short, long, default_value_t = 115200)]
    baud: u32,

    /// Default channel (0 = public)
    #[arg(short, long, default_value_t = 0)]
    channel: u8,

    /// Colour theme
    #[arg(long, value_enum, default_value_t = ThemeName::Vaporwave)]
    theme: ThemeName,
}

// ── Main ─────────────────────────────────────────────────────────────────────

/// How often to read the radio's stats; the firmware recalibrates its noise floor every 2 seconds
const STATS_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Ring the terminal bell
fn bell() {
    print!("\x07");
}

/// Apply a message, then carry out the effects it calls for; radio work goes to the background task
fn dispatch(app: &mut App, msg: Msg, radio: Option<&UnboundedSender<Effect>>) {
    for effect in update(app, msg) {
        match (effect, radio) {
            (Effect::Alert, _) => bell(),
            (work, Some(radio)) => {
                if radio.send(work).is_err() {
                    app.err("Radio task stopped");
                }
            }
            (_, None) => app.err("Not connected"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Tracing: suppress all output by default (TUI uses the terminal).
    // Set MESHCORE_LOG=1 to write debug logs to meshcore-chat.log
    if std::env::var("MESHCORE_LOG").is_ok() {
        let log_file = std::fs::File::create("meshcore-chat.log").ok();
        if let Some(file) = log_file {
            tracing_subscriber::fmt()
                .with_env_filter("meshcore_rs=debug,btleplug=warn")
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .init();
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(args.channel);
    app.theme = args.theme.theme();
    app.sys("MeshCore Chat — Rust TUI Client v0.1");
    app.sys("Tab=panel  ↑↓=contact  F1=deselect  Enter=send  /help=commands  Esc=quit");

    let mc = connect(&mut app, &args).await.map(Arc::new);
    let (radio_events_tx, mut radio_events) = mpsc::unbounded_channel();
    let (outcomes_tx, mut outcomes) = mpsc::unbounded_channel();
    let mut radio = None;

    if let Some(ref mc) = mc {
        app.sys("Ready! Type a message and press Enter.");
        setup_subscriptions(mc, radio_events_tx).await;
        let (effects_tx, effects_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_radio_effects(Arc::clone(mc), effects_rx, outcomes_tx));
        radio = Some(effects_tx);
    } else {
        app.sys("Offline mode — fix connection and restart.");
    }

    // Main loop
    let mut next_stats_poll = Instant::now();
    loop {
        while let Ok(payload) = radio_events.try_recv() {
            dispatch(&mut app, Msg::Radio(payload), radio.as_ref());
        }
        while let Ok(outcome) = outcomes.try_recv() {
            dispatch(&mut app, Msg::EffectDone(outcome), radio.as_ref());
        }
        if Instant::now() >= next_stats_poll {
            next_stats_poll = Instant::now() + STATS_POLL_INTERVAL;
            dispatch(&mut app, Msg::StatsPollDue, radio.as_ref());
        }

        terminal.draw(|f| draw(f, &app))?;

        if event::poll(Duration::from_millis(50))? {
            if let CEvent::Key(key) = event::read()? {
                dispatch(&mut app, Msg::Key(key), radio.as_ref());
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Cleanup
    if let Some(mc) = mc {
        let _ = mc.disconnect().await;
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    println!("MeshCore Chat terminated.");
    Ok(())
}
