use std::io;
use std::time::Duration;

use clap::Parser;
use color_eyre::eyre::WrapErr;
use crossterm::event::{poll, read, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

mod action;
mod app;
mod components;
mod event;

use app::App;

#[derive(Parser)]
#[command(
    name = "ogexplain-tui",
    version,
    about = "Interactive EXPLAIN plan analyzer"
)]
struct Cli {
    /// EXPLAIN output file to load on startup
    file: Option<String>,
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    enable_raw_mode().wrap_err("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).wrap_err("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).wrap_err("failed to create terminal")?;

    let mut app = App::new();

    if let Some(file) = cli.file {
        if let Err(e) = app.load_file(&file) {
            app = App::new();
            app.set_error(format!("加载文件 '{}' 失败: {}", file, e));
        }
    }

    let res = run_app(&mut terminal, &mut app);

    disable_raw_mode().wrap_err("failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .wrap_err("failed to leave alternate screen")?;
    terminal.show_cursor().wrap_err("failed to show cursor")?;

    res
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> color_eyre::Result<()> {
    loop {
        terminal.draw(|f| app.draw(f)).wrap_err("draw failed")?;

        if poll(Duration::from_millis(100)).wrap_err("poll failed")? {
            match read().wrap_err("event read failed")? {
                Event::Key(key) => {
                    app.handle_key(key);
                }
                Event::Paste(text) => {
                    app.handle_paste(&text);
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
