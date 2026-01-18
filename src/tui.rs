use crate::api::{ReplyThread, Thread, ThreadsClient};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    DefaultTerminal, Frame,
};
use std::io::{self, stdout};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Threads,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Replying,
    Posting,
}

pub enum AppEvent {
    ThreadsUpdated(Vec<Thread>),
    ReplyResult(Result<(), String>),
    PostResult(Result<(), String>),
    RepliesLoaded(String, Result<Vec<ReplyThread>, String>), // (thread_id, nested replies or error)
}

pub struct App {
    pub running: bool,
    pub active_panel: Panel,
    pub threads: Vec<Thread>,
    pub list_state: ListState,
    pub show_help: bool,
    pub swapped_layout: bool,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub status_message: Option<String>,
    pub client: ThreadsClient,
    pub event_rx: mpsc::Receiver<AppEvent>,
    pub event_tx: mpsc::Sender<AppEvent>,
    pub selected_replies: Vec<ReplyThread>,
    pub loaded_replies_for: Option<String>, // thread_id we've loaded replies for
}

impl App {
    pub fn new(client: ThreadsClient, threads: Vec<Thread>) -> Self {
        let mut state = ListState::default();
        if !threads.is_empty() {
            state.select(Some(0));
        }

        let (event_tx, event_rx) = mpsc::channel(32);

        Self {
            running: true,
            active_panel: Panel::Threads,
            threads,
            list_state: state,
            show_help: false,
            swapped_layout: false,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            status_message: None,
            client,
            event_rx,
            event_tx,
            selected_replies: Vec::new(),
            loaded_replies_for: None,
        }
    }

    pub async fn run(&mut self) -> io::Result<()> {
        stdout().execute(EnterAlternateScreen)?;
        enable_raw_mode()?;

        let mut terminal = ratatui::init();
        terminal.clear()?;

        // Start background refresh
        self.start_refresh_task();

        let result = self.main_loop(&mut terminal).await;

        stdout().execute(LeaveAlternateScreen)?;
        disable_raw_mode()?;

        result
    }

    fn start_refresh_task(&self) {
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;

                if let Ok(resp) = client.get_threads(Some(25)).await {
                    let _ = tx.send(AppEvent::ThreadsUpdated(resp.data)).await;
                }
            }
        });
    }

    async fn main_loop(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while self.running {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events().await?;
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(frame.area());

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(main_chunks[0]);

        if self.swapped_layout {
            self.draw_detail(frame, chunks[0]);
            self.draw_threads_list(frame, chunks[1]);
        } else {
            self.draw_threads_list(frame, chunks[0]);
            self.draw_detail(frame, chunks[1]);
        }

        self.draw_status_bar(frame, main_chunks[1]);

        if self.show_help {
            self.draw_help(frame);
        }

        if self.input_mode == InputMode::Replying || self.input_mode == InputMode::Posting {
            self.draw_input(frame);
        }
    }

    fn draw_status_bar(&self, frame: &mut Frame, area: Rect) {
        let status = self
            .status_message
            .as_deref()
            .unwrap_or("? for help | p to post | r to reply | R to refresh");

        let style = if self.status_message.is_some() {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let paragraph = Paragraph::new(status)
            .style(style)
            .block(Block::default().borders(Borders::ALL));

        frame.render_widget(paragraph, area);
    }

    fn draw_input(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = 60.min(area.width.saturating_sub(4));
        let popup_height = 5;
        let popup_area = Rect {
            x: area.width.saturating_sub(popup_width) / 2,
            y: area.height.saturating_sub(popup_height) / 2,
            width: popup_width,
            height: popup_height,
        };

        frame.render_widget(Clear, popup_area);

        let title = match self.input_mode {
            InputMode::Replying => " Reply (Enter to send, Esc to cancel) ",
            InputMode::Posting => " New Post (Enter to send, Esc to cancel) ",
            InputMode::Normal => "",
        };

        let input = Paragraph::new(self.input_buffer.as_str())
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(input, popup_area);
    }

    fn draw_help(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = 42;
        let popup_height = 17;
        let popup_area = Rect {
            x: area.width.saturating_sub(popup_width) / 2,
            y: area.height.saturating_sub(popup_height) / 2,
            width: popup_width.min(area.width),
            height: popup_height.min(area.height),
        };

        let help_text = "\
j / Down     Move down
k / Up       Move up
h / Left     Focus left panel
l / Right    Focus right panel
t            Swap panel positions
p            Create new post
r            Reply to selected thread
R            Refresh threads
Enter        Select item
Esc          Back / Cancel
q            Quit
?            Toggle help";

        frame.render_widget(Clear, popup_area);
        let help = Paragraph::new(help_text)
            .block(
                Block::default()
                    .title(" Help ")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .alignment(Alignment::Left);

        frame.render_widget(help, popup_area);
    }

    fn draw_threads_list(&mut self, frame: &mut Frame, area: Rect) {
        let is_active = self.active_panel == Panel::Threads;
        let border_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let items: Vec<ListItem> = self
            .threads
            .iter()
            .map(|t| {
                let display = if let Some(text) = t.text.as_deref() {
                    let truncated: String = text.chars().take(50).collect();
                    if text.len() > 50 {
                        format!("{}...", truncated)
                    } else {
                        truncated
                    }
                } else {
                    // No text - show media type indicator
                    match t.media_type.as_deref() {
                        Some("REPOST_FACADE") => "[repost]".to_string(),
                        Some("IMAGE") => "[image]".to_string(),
                        Some("VIDEO") => "[video]".to_string(),
                        Some("CAROUSEL_ALBUM") => "[carousel]".to_string(),
                        Some(other) => format!("[{}]", other.to_lowercase()),
                        None => "[no text]".to_string(),
                    }
                };
                ListItem::new(Line::from(display))
            })
            .collect();

        let title = format!(" Threads ({}) ", self.threads.len());
        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn draw_detail(&self, frame: &mut Frame, area: Rect) {
        let is_active = self.active_panel == Panel::Detail;
        let border_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let content = if let Some(idx) = self.list_state.selected() {
            if let Some(thread) = self.threads.get(idx) {
                let username = thread.username.as_deref().unwrap_or("unknown");
                let timestamp = thread.timestamp.as_deref().unwrap_or("");
                let media_type = thread.media_type.as_deref().unwrap_or("TEXT_POST");

                let text = if let Some(t) = thread.text.as_deref() {
                    t.to_string()
                } else {
                    match media_type {
                        "REPOST_FACADE" => {
                            let link = thread.permalink.as_deref().unwrap_or("");
                            format!("[Repost]\n{}", link)
                        }
                        "IMAGE" => "[Image post]".to_string(),
                        "VIDEO" => "[Video post]".to_string(),
                        "CAROUSEL_ALBUM" => "[Carousel post]".to_string(),
                        _ => "[No text]".to_string(),
                    }
                };

                let mut content = format!("@{}\n{}\n\n{}", username, timestamp, text);

                // Add replies section
                if !self.selected_replies.is_empty() {
                    content.push_str("\n\n--- Replies ---\n");
                    fn format_replies(replies: &[ReplyThread], indent: usize, out: &mut String) {
                        let prefix = "  ".repeat(indent);
                        for reply in replies {
                            let user = reply.thread.username.as_deref().unwrap_or("unknown");
                            let text = reply.thread.text.as_deref().unwrap_or("[no text]");
                            out.push_str(&format!("\n{}@{}: {}\n", prefix, user, text));
                            if !reply.replies.is_empty() {
                                format_replies(&reply.replies, indent + 1, out);
                            }
                        }
                    }
                    format_replies(&self.selected_replies, 0, &mut content);
                } else if self.loaded_replies_for.as_ref() == Some(&thread.id) {
                    content.push_str("\n\n--- No replies ---");
                } else {
                    content.push_str("\n\n--- Loading replies... ---");
                }

                content
            } else {
                "No thread selected".to_string()
            }
        } else {
            "No thread selected".to_string()
        };

        let paragraph = Paragraph::new(content)
            .block(
                Block::default()
                    .title(" Detail ")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }

    async fn handle_events(&mut self) -> io::Result<()> {
        // Check for app events (refresh, reply results)
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AppEvent::ThreadsUpdated(threads) => {
                    self.threads = threads;
                    if self.list_state.selected().is_none() && !self.threads.is_empty() {
                        self.list_state.select(Some(0));
                    }
                    self.status_message = Some("Refreshed".to_string());
                }
                AppEvent::ReplyResult(result) => {
                    match result {
                        Ok(()) => self.status_message = Some("Reply sent!".to_string()),
                        Err(e) => self.status_message = Some(format!("Error: {}", e)),
                    }
                }
                AppEvent::PostResult(result) => {
                    match result {
                        Ok(()) => {
                            self.status_message = Some("Post sent!".to_string());
                            // Refresh to show the new post
                            self.refresh_threads().await;
                        }
                        Err(e) => self.status_message = Some(format!("Error: {}", e)),
                    }
                }
                AppEvent::RepliesLoaded(thread_id, result) => {
                    self.loaded_replies_for = Some(thread_id);
                    match result {
                        Ok(replies) => self.selected_replies = replies,
                        Err(e) => {
                            self.selected_replies = Vec::new();
                            self.status_message = Some(format!("Replies: {}", e));
                        }
                    }
                }
            }
        }

        // Check if we need to load replies for current selection
        self.maybe_load_replies();

        // Handle keyboard
        if event::poll(std::time::Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Clear status on any key
                    self.status_message = None;

                    match self.input_mode {
                        InputMode::Replying | InputMode::Posting => self.handle_input_mode(key.code).await,
                        InputMode::Normal => self.handle_normal_input(key.code).await,
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_input_mode(&mut self, key: KeyCode) {
        match key {
            KeyCode::Enter => {
                if !self.input_buffer.is_empty() {
                    match self.input_mode {
                        InputMode::Replying => self.send_reply().await,
                        InputMode::Posting => self.send_post().await,
                        InputMode::Normal => {}
                    }
                }
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
    }

    async fn handle_normal_input(&mut self, key: KeyCode) {
        if self.show_help {
            self.show_help = false;
            return;
        }

        match key {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('t') => self.toggle_panel(),
            KeyCode::Char('r') => self.start_reply(),
            KeyCode::Char('p') => self.start_post(),
            KeyCode::Char('R') => self.refresh_threads().await,
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('h') | KeyCode::Left => self.move_left(),
            KeyCode::Char('l') | KeyCode::Right => self.move_right(),
            KeyCode::Enter => self.select_item(),
            KeyCode::Esc => self.deselect(),
            _ => {}
        }
    }

    fn start_reply(&mut self) {
        if self.list_state.selected().is_some() {
            self.input_mode = InputMode::Replying;
            self.input_buffer.clear();
        }
    }

    fn start_post(&mut self) {
        self.input_mode = InputMode::Posting;
        self.input_buffer.clear();
    }

    async fn send_reply(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            if let Some(thread) = self.threads.get(idx) {
                let thread_id = thread.id.clone();
                let text = self.input_buffer.clone();
                let client = self.client.clone();
                let tx = self.event_tx.clone();

                self.status_message = Some("Sending reply...".to_string());

                tokio::spawn(async move {
                    let result = client.reply_to_thread(&thread_id, &text).await;
                    let _ = tx
                        .send(AppEvent::ReplyResult(result.map(|_| ()).map_err(|e| e.to_string())))
                        .await;
                });
            }
        }
    }

    async fn send_post(&mut self) {
        let text = self.input_buffer.clone();
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        self.status_message = Some("Posting...".to_string());

        tokio::spawn(async move {
            let result = client.post_thread(&text).await;
            let _ = tx
                .send(AppEvent::PostResult(result.map(|_| ()).map_err(|e| e.to_string())))
                .await;
        });
    }

    async fn refresh_threads(&mut self) {
        self.status_message = Some("Refreshing...".to_string());
        match self.client.get_threads(Some(25)).await {
            Ok(resp) => {
                self.threads = resp.data;
                self.status_message = Some("Refreshed".to_string());
            }
            Err(e) => {
                self.status_message = Some(format!("Refresh failed: {}", e));
            }
        }
    }

    fn maybe_load_replies(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            if let Some(thread) = self.threads.get(idx) {
                // Check if we already loaded replies for this thread
                if self.loaded_replies_for.as_ref() != Some(&thread.id) {
                    let thread_id = thread.id.clone();
                    let client = self.client.clone();
                    let tx = self.event_tx.clone();

                    // Clear old replies while loading
                    self.selected_replies.clear();
                    self.loaded_replies_for = None;

                    tokio::spawn(async move {
                        let result = client.get_thread_replies_nested(&thread_id, 2) // 2 levels deep
                            .await
                            .map_err(|e| e.to_string());
                        let _ = tx.send(AppEvent::RepliesLoaded(thread_id, result)).await;
                    });
                }
            }
        }
    }

    fn move_down(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.threads.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn move_up(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.threads.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn move_left(&mut self) {
        self.active_panel = Panel::Threads;
    }

    fn move_right(&mut self) {
        self.active_panel = Panel::Detail;
    }

    fn toggle_panel(&mut self) {
        self.swapped_layout = !self.swapped_layout;
    }

    fn select_item(&mut self) {
        self.active_panel = Panel::Detail;
    }

    fn deselect(&mut self) {
        self.active_panel = Panel::Threads;
    }
}
