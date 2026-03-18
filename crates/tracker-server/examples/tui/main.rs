mod app;
mod ui;

use app::App;
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use eyre::Result;
use futures_util::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use signet_tracker::OrderStatus;
use std::io::stdout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<()> {
    let url =
        std::env::args().nth(1).unwrap_or_else(|| "ws://localhost:8019/orders/ws".to_string());

    eprintln!("Connecting to {url}…");
    let (ws_stream, _) = connect_async(&url).await?;

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run(&mut terminal, ws_stream).await;

    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;

    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Result<()> {
    let mut app = App::new();
    let mut events = EventStream::new();
    let (_write, mut read) = ws_stream.split();

    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        if !app.running {
            break;
        }

        tokio::select! {
            event = events.next() => {
                let Some(Ok(event)) = event else { break };
                if let Event::Key(key) = event
                    && key.kind == KeyEventKind::Press
                {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.running = false,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.running = false;
                        }
                        KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
                        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
                        _ => {}
                    }
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(status) = serde_json::from_str::<OrderStatus>(&text) {
                            app.update_order(status);
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                        app.connected = false;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
