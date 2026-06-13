//! sndlab: a TUI sound-design environment.

mod app;
mod log;
mod syntax;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::App;

fn main() -> Result<()> {
    // Tracing goes to stderr by default. We swap to a file so it doesn't
    // collide with the alternate-screen TUI on stdout.
    let _ = tracing_subscriber::fmt()
        .with_writer(io::sink)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let mut terminal = setup_terminal().context("terminal setup")?;
    let result = run(&mut terminal);
    restore_terminal(&mut terminal).context("terminal restore")?;
    result
}

type TuiTerminal = Terminal<CrosstermBackend<io::Stdout>>;

fn setup_terminal() -> Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut TuiTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn run(terminal: &mut TuiTerminal) -> Result<()> {
    let mut app = App::new();
    // Tick cadence: keep the UI responsive (~30 Hz redraw budget) while
    // still letting `poll` block when there's nothing happening.
    let tick = Duration::from_millis(33);
    loop {
        terminal.draw(|f| ui::render(f, &app))?;
        if crossterm::event::poll(tick)? {
            match crossterm::event::read()? {
                Event::Key(key) => {
                    app.on_input(key.into());
                    if app.should_quit {
                        break;
                    }
                }
                Event::Resize(_, _) => {
                    // Next draw call picks up the new size; nothing to do.
                }
                _ => {}
            }
        }
    }
    Ok(())
}
