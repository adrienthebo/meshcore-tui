mod app;
mod choices;
mod commands;
mod contact_menu;
mod contacts_tab;
mod input;
mod overview;
mod popover;
mod radio;
mod radio_log;
mod radio_tab;
mod rx_log;
mod rx_log_tab;
mod stats;
mod tab_bar;
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
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

use app::App;
use radio::{connect, run_radio_effects, setup_subscriptions};
use radio_log::RadioLog;
use theme::ThemeName;
use ui::draw;
use update::{update, Effect, Msg};
use view::Ui;

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "meshcore-chat",
    about = "TUI chat for MeshCore mesh networking"
)]
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
fn dispatch(app: &mut App, ui: &mut Ui, msg: Msg, radio: Option<&UnboundedSender<Effect>>) {
    for effect in update(app, ui, msg) {
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

    // Tracing: nothing goes to the terminal, which the TUI uses. meshcore-rs
    // warnings and errors go to the message log.
    // Set MESHCORE_LOG=1 to also write debug logs to meshcore-chat.log
    let log_file = std::env::var("MESHCORE_LOG")
        .ok()
        .and_then(|_| std::fs::File::create("meshcore-chat.log").ok())
        .map(|file| {
            tracing_subscriber::fmt::layer()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .with_filter(EnvFilter::new("meshcore_rs=debug,btleplug=warn"))
        });
    let (radio_log_tx, mut radio_log) = mpsc::unbounded_channel();
    tracing_subscriber::registry()
        .with(log_file)
        .with(RadioLog::new(radio_log_tx).with_filter(radio_log::shown()))
        .init();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(args.channel);
    let mut ui = Ui::new(args.theme.theme());
    app.sys("MeshCore Chat — Rust TUI Client v0.1");
    app.sys("Keys are on the tab bar  /help=commands");

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
            dispatch(&mut app, &mut ui, Msg::Radio(payload), radio.as_ref());
        }
        while let Ok(outcome) = outcomes.try_recv() {
            dispatch(&mut app, &mut ui, Msg::EffectDone(outcome), radio.as_ref());
        }
        while let Ok(line) = radio_log.try_recv() {
            dispatch(&mut app, &mut ui, Msg::RadioLog(line), radio.as_ref());
        }
        if Instant::now() >= next_stats_poll {
            next_stats_poll = Instant::now() + STATS_POLL_INTERVAL;
            dispatch(&mut app, &mut ui, Msg::StatsPollDue, radio.as_ref());
        }

        terminal.draw(|f| draw(f, &app, &ui))?;

        if event::poll(Duration::from_millis(50))? {
            if let CEvent::Key(key) = event::read()? {
                dispatch(&mut app, &mut ui, Msg::Key(key), radio.as_ref());
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
