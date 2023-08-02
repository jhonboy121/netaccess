use crate::{account_manager::SystemStatus, monitor::State};
use anyhow::bail;
use crossterm::{
    cursor::{Hide, Show},
    event::{Event, KeyCode},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    task::{self, JoinHandle},
};
use tokio_util::sync::CancellationToken;
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    widgets::{List, ListItem},
    Frame, Terminal,
};

pub fn format_duration(duration: &chrono::Duration) -> String {
    let mut fragments = Vec::with_capacity(3);
    macro_rules! push {
        ( $unit:expr ) => {
            fragments.push(format!("{} {}", $unit, stringify!($unit)));
        };
    }
    let minutes = duration.num_minutes() % 60;
    if minutes > 0 {
        push!(minutes);
    }
    let hours = duration.num_hours() % 24;
    if hours > 0 {
        push!(hours);
    }
    let days = duration.num_days();
    if days > 0 {
        push!(days);
    }
    fragments
        .into_iter()
        .rev()
        .collect::<Vec<String>>()
        .join(", ")
}

pub fn run(
    status_receiver: watch::Receiver<Option<SystemStatus>>,
    state_receiver: mpsc::Receiver<State>,
    cancellation_token: CancellationToken,
) -> JoinHandle<Result<(), anyhow::Error>> {
    tokio::spawn(async move {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, Hide)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        let res = ui_event_loop(
            &mut terminal,
            status_receiver,
            state_receiver,
            cancellation_token,
        )
        .await;
        terminal::disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen, Show)?;
        res
    })
}

#[derive(Debug, Clone, Copy)]
enum KeyInput {
    Quit,
    Wakeup,
    Retry,
}

impl TryFrom<KeyCode> for KeyInput {
    type Error = anyhow::Error;

    fn try_from(value: KeyCode) -> Result<Self, Self::Error> {
        match value {
            KeyCode::Char('q') | KeyCode::Char('Q') => Ok(Self::Quit),
            KeyCode::Char('w') | KeyCode::Char('W') => Ok(Self::Wakeup),
            KeyCode::Char('r') | KeyCode::Char('R') => Ok(Self::Retry),
            other => {
                bail!("Unknown keycode: {other:?}");
            }
        }
    }
}

struct KeyInputReader {
    handle: JoinHandle<io::Result<()>>,
    signal: Arc<AtomicBool>,
}

impl KeyInputReader {
    const POLL_DURATION: Duration = Duration::from_millis(10);

    fn new<F>(action: F) -> Self
    where
        F: FnOnce(KeyInput) + Send + 'static,
    {
        let signal: Arc<AtomicBool> = Arc::default();
        let signal_clone = Arc::clone(&signal);
        let handle = task::spawn_blocking(move || {
            while !signal_clone.load(Ordering::SeqCst) {
                if !crossterm::event::poll(Self::POLL_DURATION)? {
                    continue;
                }
                let Event::Key(key) = crossterm::event::read()? else {
                    continue;
                };
                let Ok(input) = KeyInput::try_from(key.code) else {
                    continue;
                };
                action(input);
                break;
            }
            Ok(())
        });
        Self { handle, signal }
    }

    async fn cancel(self) -> Result<io::Result<()>, task::JoinError> {
        self.signal.store(true, Ordering::SeqCst);
        self.handle.await
    }
}

async fn ui_event_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    status_receiver: watch::Receiver<Option<SystemStatus>>,
    mut state_receiver: mpsc::Receiver<State>,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()> {
    let mut key_input_reader: Option<KeyInputReader> = None;
    while let Some(state) = state_receiver.recv().await {
        if let Some(reader) = key_input_reader.take() {
            let _ = reader.cancel().await?;
        }

        terminal.draw(|frame| render_ui(frame, status_receiver.borrow().as_ref(), &state))?;

        key_input_reader = match state {
            State::Suspended {
                duration: _,
                wake_sender,
            } => {
                let cancellation_token = cancellation_token.clone();
                KeyInputReader::new(move |action| match action {
                    KeyInput::Quit => cancellation_token.cancel(),
                    KeyInput::Wakeup => {
                        let _ = wake_sender.send(());
                    }
                    _ => {}
                })
                .into()
            }
            State::Error {
                error: _,
                retry_sender,
            } => {
                let cancellation_token = cancellation_token.clone();
                KeyInputReader::new(move |action| match action {
                    KeyInput::Quit => cancellation_token.cancel(),
                    KeyInput::Retry => {
                        let _ = retry_sender.send(());
                    }
                    _ => {}
                })
                .into()
            }
            _ => None,
        };
    }
    Ok(())
}

fn render_ui<B: Backend>(frame: &mut Frame<B>, status: Option<&SystemStatus>, state: &State) {
    let rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(100)])
        .split(frame.size());

    /*
     * Max 3 status items
     * 2 for monitor state (header + text)
     * 3 for controls (header + 2 input texts)
     */
    let mut list_items = Vec::with_capacity(3 + 2 + 3);

    if let Some(status_items) = status.map(status_items) {
        list_items.extend(status_items);
    }

    list_items.push(ListItem::new("----- Monitor State -----"));
    list_items.push(state_item(state));

    let control_items = control_items(state);
    if !control_items.is_empty() {
        list_items.push(ListItem::new("----- Controls -----"));
        list_items.extend(control_items);
    }

    frame.render_widget(List::new(list_items), rects[0]);
}

fn status_items(status: &SystemStatus) -> Vec<ListItem> {
    let mut items = Vec::with_capacity(3);
    items.push(ListItem::new(format!("IP address: {}", status.ip)));
    items.push(ListItem::new(format!(
        "Connection state: {}",
        if status.connection.is_active() {
            "active"
        } else {
            "inactive"
        }
    )));
    if status.connection.is_active() {
        items.push(ListItem::new(format!(
            "Time left: {}",
            format_duration(&status.connection.time_left)
        )));
    }
    items
}

fn state_item(state: &State) -> ListItem {
    match state {
        State::Suspended {
            duration,
            wake_sender: _,
        } => ListItem::new(format!("Suspended for {duration:?}")),
        State::CheckingStatus => ListItem::new("Checking status"),
        State::Approving(ip) => ListItem::new(format!("Approving IP {ip}")),
        State::Error {
            error,
            retry_sender: _,
        } => ListItem::new(error.to_string()),
    }
}

fn control_items(state: &State) -> Vec<ListItem> {
    match state {
        State::Suspended {
            duration: _,
            wake_sender: _,
        } => {
            vec![
                ListItem::new("q | Q -> Quit monitor"),
                ListItem::new("w | W -> Wakeup monitor"),
            ]
        }
        State::CheckingStatus => vec![],
        State::Approving(_) => vec![],
        State::Error {
            error: _,
            retry_sender: _,
        } => vec![
            ListItem::new("q | Q -> Quit monitor"),
            ListItem::new("r | R -> Attempt to retry and recover"),
        ],
    }
}
