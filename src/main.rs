mod app;
mod commands;
mod input;
mod radio;
#[cfg(test)]
mod test_support;
mod text;
mod ui;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event as CEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::Mutex;

use app::App;
use commands::handle_command;
use radio::{connect, send_message, setup_subscriptions};
use ui::draw;

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
}

// ── Main ─────────────────────────────────────────────────────────────────────

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

    let app = Arc::new(Mutex::new(App::new(args.channel)));

    {
        let mut a = app.lock().await;
        a.sys("MeshCore Chat — Rust TUI Client v0.1");
        a.sys("Tab=panel  ↑↓=contact  F1=deselect  Enter=send  /help=commands  Esc=quit");
    }

    let mc = connect(&app, &args).await;

    if let Some(ref mc) = mc {
        app.lock().await.sys("Ready! Type a message and press Enter.");
        setup_subscriptions(mc, app.clone()).await;
    } else {
        app.lock()
            .await
            .sys("Offline mode — fix connection and restart.");
    }

    // Main loop
    loop {
        {
            let a = app.lock().await;
            terminal.draw(|f| draw(f, &*a))?;
        }

        if event::poll(Duration::from_millis(50))? {
            if let CEvent::Key(key) = event::read()? {
                let submitted = app.lock().await.handle_key(key);
                if let Some(text) = submitted {
                    if text.starts_with('/') {
                        handle_command(&text, &app, mc.as_ref()).await;
                    } else if let Some(ref mc) = mc {
                        send_message(mc, &app, &text).await;
                    } else {
                        app.lock().await.err("Not connected");
                    }
                }
            }
        }

        if app.lock().await.should_quit {
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
