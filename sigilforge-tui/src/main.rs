//! Sigilforge TUI - Interactive terminal interface for OAuth token management.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fusabi_tui_render::prelude::*;
use std::io::{self, stdout};
use std::time::Duration;
use tracing::{error, info};

mod app;
mod ui;

use app::App;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr) // Write logs to stderr to avoid TUI interference
        .init();

    info!("Starting Sigilforge TUI");

    // Create application
    let mut app = App::new().await?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;

    // Create renderer
    let mut renderer = CrosstermRenderer::new(stdout)?;

    // Main event loop
    let result = run_app(&mut app, &mut renderer).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    // Handle any errors from the main loop
    if let Err(e) = &result {
        error!("Application error: {}", e);
    }

    info!("Sigilforge TUI exited");
    result
}

/// Main application loop
async fn run_app(app: &mut App, renderer: &mut CrosstermRenderer<io::Stdout>) -> Result<()> {
    loop {
        // Render UI
        let buffer = ui::render(app)?;
        renderer.draw(&buffer)?;
        renderer.flush()?;

        // Handle input with timeout
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                // Only process key press events (ignore release)
                if key.kind == KeyEventKind::Press {
                    // Handle input
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            // Quit
                            break;
                        }
                        KeyCode::Char('c') | KeyCode::Char('C')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            // Ctrl+C to quit
                            break;
                        }
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            // Refresh selected account
                            app.refresh_selected().await?;
                        }
                        KeyCode::Char('a') | KeyCode::Char('A') => {
                            // Refresh all accounts
                            app.refresh_all().await?;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.select_next();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.select_previous();
                        }
                        KeyCode::Home | KeyCode::Char('g') => {
                            app.select_first();
                        }
                        KeyCode::End | KeyCode::Char('G') => {
                            app.select_last();
                        }
                        _ => {}
                    }
                }
            }
        }

        // Periodic refresh check (every 250ms)
        app.tick().await?;
    }

    Ok(())
}
