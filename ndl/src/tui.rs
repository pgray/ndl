use crate::api::{ReplyThread, Thread, ThreadsClient};
use crate::platform::{Platform, Post, ReplyThread as PlatformReplyThread, SocialClient};
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::collections::HashMap;
use std::io::{self, stdout};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

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
    CrossPosting, // Post to all platforms
}

pub enum AppEvent {
    // Legacy events for backwards compatibility
    ThreadsUpdated(Vec<Thread>),
    ReplyResult(Result<(), String>),
    PostResult(Result<(), String>),
    RepliesLoaded(String, Result<Vec<ReplyThread>, String>), // (thread_id, nested replies or error)

    // Platform-aware events
    PostsUpdated(Platform, Vec<Post>),
    PlatformReplyResult(Platform, Result<(), String>),
    PlatformPostResult(Platform, Result<(), String>),
    PlatformRepliesLoaded(Platform, String, Result<Vec<PlatformReplyThread>, String>),
}

/// Platform-specific state
pub struct PlatformState {
    pub posts: Vec<Post>,
    pub list_state: ListState,
    pub selected_replies: Vec<PlatformReplyThread>,
    pub loaded_replies_for: Option<String>,
    pub reply_selection: Option<usize>,
}

impl PlatformState {
    fn new() -> Self {
        Self {
            posts: Vec::new(),
            list_state: ListState::default(),
            selected_replies: Vec::new(),
            loaded_replies_for: None,
            reply_selection: None,
        }
    }
}

pub struct App {
    pub running: bool,
    pub active_panel: Panel,
    pub show_help: bool,
    pub swapped_layout: bool,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub status_message: Option<String>,
    pub event_rx: mpsc::Receiver<AppEvent>,
    pub event_tx: mpsc::Sender<AppEvent>,

    // Multi-platform support
    pub current_platform: Platform,
    pub clients: HashMap<Platform, Arc<Box<dyn SocialClient>>>,
    pub platform_states: HashMap<Platform, PlatformState>,

    // Legacy fields for backwards compatibility
    pub threads: Vec<Thread>,
    pub list_state: ListState,
    pub client: ThreadsClient,
    pub selected_replies: Vec<ReplyThread>,
    pub loaded_replies_for: Option<String>,
    pub reply_selection: Option<usize>,
}

impl App {
    pub fn new(client: ThreadsClient, threads: Vec<Thread>) -> Self {
        let mut state = ListState::default();
        if !threads.is_empty() {
            state.select(Some(0));
        }

        let (event_tx, event_rx) = mpsc::channel(32);

        // Initialize with empty multi-platform support
        let clients = HashMap::new();
        let platform_states = HashMap::new();

        Self {
            running: true,
            active_panel: Panel::Threads,
            show_help: false,
            swapped_layout: false,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            status_message: None,
            event_rx,
            event_tx,
            current_platform: Platform::Threads,
            clients,
            platform_states,
            // Legacy fields
            threads,
            list_state: state,
            client,
            selected_replies: Vec::new(),
            loaded_replies_for: None,
            reply_selection: None,
        }
    }

    /// Create a new multi-platform app with clients for each platform
    pub fn new_multi_platform(clients: HashMap<Platform, Box<dyn SocialClient>>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(32);

        let mut platform_states = HashMap::new();
        let mut clients_arc = HashMap::new();

        // Initialize state for each platform
        for (platform, client) in clients {
            platform_states.insert(platform, PlatformState::new());
            clients_arc.insert(platform, Arc::new(client));
        }

        // Pick the first platform as default
        let current_platform = clients_arc
            .keys()
            .next()
            .copied()
            .unwrap_or(Platform::Threads);

        // Create a dummy ThreadsClient for legacy compatibility
        let legacy_client = ThreadsClient::new(String::new());

        Self {
            running: true,
            active_panel: Panel::Threads,
            show_help: false,
            swapped_layout: false,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            status_message: None,
            event_rx,
            event_tx,
            current_platform,
            clients: clients_arc,
            platform_states,
            // Legacy fields
            threads: Vec::new(),
            list_state: ListState::default(),
            client: legacy_client,
            selected_replies: Vec::new(),
            loaded_replies_for: None,
            reply_selection: None,
        }
    }

    /// Get the current platform's state
    fn current_state(&self) -> Option<&PlatformState> {
        self.platform_states.get(&self.current_platform)
    }

    /// Get the current platform's state (mutable)
    fn current_state_mut(&mut self) -> Option<&mut PlatformState> {
        self.platform_states.get_mut(&self.current_platform)
    }

    /// Toggle to the next platform
    fn toggle_platform(&mut self) {
        let platforms: Vec<Platform> = self.clients.keys().copied().collect();
        if platforms.len() <= 1 {
            return;
        }

        let current_idx = platforms
            .iter()
            .position(|p| *p == self.current_platform)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % platforms.len();
        self.current_platform = platforms[next_idx];

        self.status_message = Some(format!("Switched to {}", self.current_platform));
    }

    pub async fn run(&mut self) -> io::Result<()> {
        stdout().execute(EnterAlternateScreen)?;
        enable_raw_mode()?;

        let mut terminal = ratatui::init();
        terminal.clear()?;

        // Fetch initial data for all platforms
        self.fetch_initial_data().await;

        // Start background refresh
        self.start_refresh_task();

        let result = self.main_loop(&mut terminal).await;

        stdout().execute(LeaveAlternateScreen)?;
        disable_raw_mode()?;

        result
    }

    async fn fetch_initial_data(&mut self) {
        self.status_message = Some("Loading...".to_string());

        // Fetch data for all platforms in multi-platform mode
        for (platform, client) in &self.clients {
            let platform = *platform;
            debug!("Fetching initial data for {}", platform);
            match client.get_posts(Some(25)).await {
                Ok(posts) => {
                    debug!("Initial fetch: {} posts for {}", posts.len(), platform);
                    if let Some(state) = self.platform_states.get_mut(&platform) {
                        state.posts = posts;
                        if !state.posts.is_empty() {
                            state.list_state.select(Some(0));
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to fetch initial data for {}: {}", platform, e);
                }
            }
        }

        // Also fetch for legacy Threads mode if we have threads data (but no multi-platform client for it)
        if self.threads.is_empty() && !self.clients.contains_key(&Platform::Threads) {
            if let Ok(resp) = self.client.get_threads(Some(25)).await {
                debug!("Initial fetch: {} threads (legacy)", resp.data.len());
                self.threads = resp.data;
                if !self.threads.is_empty() && self.list_state.selected().is_none() {
                    self.list_state.select(Some(0));
                }
            }
        }

        self.status_message = None;
    }

    fn start_refresh_task(&self) {
        // Start Threads refresh task
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

        // Start refresh tasks for other platforms
        for (platform, client) in &self.clients {
            let platform = *platform;
            let client = client.clone();
            let tx = self.event_tx.clone();

            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;

                    if let Ok(posts) = client.get_posts(Some(25)).await {
                        let _ = tx.send(AppEvent::PostsUpdated(platform, posts)).await;
                    }
                }
            });
        }
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

        if self.input_mode == InputMode::Replying
            || self.input_mode == InputMode::Posting
            || self.input_mode == InputMode::CrossPosting
        {
            self.draw_input(frame);
        }
    }

    fn draw_status_bar(&self, frame: &mut Frame, area: Rect) {
        let mut status = self
            .status_message
            .as_deref()
            .unwrap_or("? for help | p to post | r to reply | R to refresh")
            .to_string();

        // Add platform indicator if multi-platform mode is active
        if !self.clients.is_empty() {
            let platforms: Vec<String> = self
                .clients
                .keys()
                .map(|p| {
                    if *p == self.current_platform {
                        format!("[{}]", p) // Active platform in brackets
                    } else {
                        p.to_string()
                    }
                })
                .collect();
            let platform_str = platforms.join(" ");
            status = format!("{} | {}", platform_str, status);
        }

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
            InputMode::CrossPosting => " Cross-Post to All (Enter to send, Esc to cancel) ",
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
        let popup_width = 48;
        let popup_height = 19;
        let popup_area = Rect {
            x: area.width.saturating_sub(popup_width) / 2,
            y: area.height.saturating_sub(popup_height) / 2,
            width: popup_width.min(area.width),
            height: popup_height.min(area.height),
        };

        let help_text = "\
j / Down     Move down (or select reply)
k / Up       Move up (or select reply)
h / Left     Focus left panel
l / Right    Focus right panel
t            Swap panel positions
p            Create new post
P            Cross-post to all platforms
r            Reply to thread or reply
R            Refresh threads
] / Tab      Switch platform (multi-platform)
Enter        Select item
Esc          Back / Cancel / Deselect
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

        // Check if we're using multi-platform mode for current platform
        if let Some(state) = self.platform_states.get(&self.current_platform) {
            let items: Vec<ListItem> = state
                .posts
                .iter()
                .map(|p| {
                    let display = if let Some(text) = p.text.as_deref() {
                        let truncated: String = text.chars().take(50).collect();
                        if text.len() > 50 {
                            format!("{}...", truncated)
                        } else {
                            truncated
                        }
                    } else {
                        "[no text]".to_string()
                    };
                    ListItem::new(Line::from(display))
                })
                .collect();

            let title = format!(" {} ({}) ", self.current_platform, state.posts.len());
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

            // Need to get mutable reference to list_state
            if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
                frame.render_stateful_widget(list, area, &mut state.list_state);
            }
        } else {
            // Legacy Threads mode
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
    }

    fn draw_detail(&self, frame: &mut Frame, area: Rect) {
        let is_active = self.active_panel == Panel::Detail;
        let border_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Check if we're using multi-platform mode
        let content = if let Some(state) = self.platform_states.get(&self.current_platform) {
            if let Some(idx) = state.list_state.selected() {
                if let Some(post) = state.posts.get(idx) {
                    let author = post.author_handle.as_deref().unwrap_or("unknown");
                    let timestamp = post.timestamp.as_deref().unwrap_or("");
                    let text = post.text.as_deref().unwrap_or("[no text]");

                    let mut content = format!("@{}\n{}\n\n{}", author, timestamp, text);

                    // Add replies section
                    if !state.selected_replies.is_empty() {
                        content.push_str("\n\n--- Replies (j/k to select, r to reply) ---\n");
                        let selected_idx = state.reply_selection;
                        fn format_platform_replies(
                            replies: &[PlatformReplyThread],
                            indent: usize,
                            out: &mut String,
                            counter: &mut usize,
                            selected: Option<usize>,
                        ) {
                            let prefix = "  ".repeat(indent);
                            for reply in replies {
                                let user = reply.post.author_handle.as_deref().unwrap_or("unknown");
                                let text = reply.post.text.as_deref().unwrap_or("[no text]");
                                let marker = if selected == Some(*counter) {
                                    "> "
                                } else {
                                    "  "
                                };
                                out.push_str(&format!("\n{}{}@{}: {}\n", marker, prefix, user, text));
                                *counter += 1;
                                if !reply.replies.is_empty() {
                                    format_platform_replies(&reply.replies, indent + 1, out, counter, selected);
                                }
                            }
                        }
                        let mut counter = 0;
                        format_platform_replies(&state.selected_replies, 0, &mut content, &mut counter, selected_idx);
                    } else if state.loaded_replies_for.as_ref() == Some(&post.id) {
                        content.push_str("\n\n--- No replies ---");
                    } else {
                        content.push_str("\n\n--- Loading replies... ---");
                    }

                    content
                } else {
                    "No post selected".to_string()
                }
            } else {
                "No post selected".to_string()
            }
        } else if let Some(idx) = self.list_state.selected() {
            // Legacy Threads mode
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
                    content.push_str("\n\n--- Replies (j/k to select, r to reply) ---\n");
                    let selected_idx = self.reply_selection;
                    fn format_replies(
                        replies: &[ReplyThread],
                        indent: usize,
                        out: &mut String,
                        counter: &mut usize,
                        selected: Option<usize>,
                    ) {
                        let prefix = "  ".repeat(indent);
                        for reply in replies {
                            let user = reply.thread.username.as_deref().unwrap_or("unknown");
                            let text = reply.thread.text.as_deref().unwrap_or("[no text]");
                            let marker = if selected == Some(*counter) {
                                "> "
                            } else {
                                "  "
                            };
                            out.push_str(&format!("\n{}{}@{}: {}\n", marker, prefix, user, text));
                            *counter += 1;
                            if !reply.replies.is_empty() {
                                format_replies(&reply.replies, indent + 1, out, counter, selected);
                            }
                        }
                    }
                    let mut counter = 0;
                    format_replies(
                        &self.selected_replies,
                        0,
                        &mut content,
                        &mut counter,
                        selected_idx,
                    );
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
                    debug!("Threads updated: {} threads", threads.len());
                    self.threads = threads;
                    if self.list_state.selected().is_none() && !self.threads.is_empty() {
                        self.list_state.select(Some(0));
                    }
                    self.status_message = Some("Refreshed".to_string());
                }
                AppEvent::ReplyResult(result) => match result {
                    Ok(()) => {
                        info!("Reply sent successfully");
                        self.status_message = Some("Reply sent!".to_string());
                    }
                    Err(ref e) => {
                        error!("Reply failed: {}", e);
                        self.status_message = Some(format!("Error: {}", e));
                    }
                },
                AppEvent::PostResult(result) => {
                    match result {
                        Ok(()) => {
                            info!("Post sent successfully");
                            self.status_message = Some("Post sent!".to_string());
                            // Refresh to show the new post
                            self.refresh_threads().await;
                        }
                        Err(ref e) => {
                            error!("Post failed: {}", e);
                            self.status_message = Some(format!("Error: {}", e));
                        }
                    }
                }
                AppEvent::RepliesLoaded(thread_id, result) => {
                    self.loaded_replies_for = Some(thread_id.clone());
                    match result {
                        Ok(replies) => {
                            debug!("Loaded {} replies for thread {}", replies.len(), thread_id);
                            self.selected_replies = replies;
                        }
                        Err(ref e) => {
                            error!("Failed to load replies for {}: {}", thread_id, e);
                            self.selected_replies = Vec::new();
                            self.status_message = Some(format!("Replies: {}", e));
                        }
                    }
                }
                AppEvent::PostsUpdated(platform, posts) => {
                    debug!("Received {} posts for {}", posts.len(), platform);
                    if let Some(state) = self.platform_states.get_mut(&platform) {
                        state.posts = posts;
                        if state.list_state.selected().is_none() && !state.posts.is_empty() {
                            state.list_state.select(Some(0));
                        }
                    }
                    if platform == self.current_platform {
                        self.status_message = Some(format!("{} refreshed", platform));
                    }
                }
                AppEvent::PlatformPostResult(platform, result) => match result {
                    Ok(()) => {
                        info!("Post sent successfully to {}", platform);
                        self.status_message = Some(format!("Posted to {}!", platform));
                    }
                    Err(ref e) => {
                        error!("Post to {} failed: {}", platform, e);
                        self.status_message = Some(format!("{} error: {}", platform, e));
                    }
                },
                AppEvent::PlatformReplyResult(platform, result) => match result {
                    Ok(()) => {
                        info!("Reply sent successfully to {}", platform);
                        self.status_message = Some(format!("Replied on {}!", platform));
                    }
                    Err(ref e) => {
                        error!("Reply to {} failed: {}", platform, e);
                        self.status_message = Some(format!("{} error: {}", platform, e));
                    }
                },
                AppEvent::PlatformRepliesLoaded(platform, post_id, result) => {
                    if let Some(state) = self.platform_states.get_mut(&platform) {
                        state.loaded_replies_for = Some(post_id.clone());
                        match result {
                            Ok(replies) => {
                                debug!(
                                    "Loaded {} replies for {} post {}",
                                    replies.len(),
                                    platform,
                                    post_id
                                );
                                state.selected_replies = replies;
                            }
                            Err(ref e) => {
                                error!(
                                    "Failed to load replies for {} post {}: {}",
                                    platform, post_id, e
                                );
                                state.selected_replies = Vec::new();
                                self.status_message = Some(format!("Replies: {}", e));
                            }
                        }
                    }
                }
            }
        }

        // Check if we need to load replies for current selection
        self.maybe_load_replies();

        // Handle keyboard
        if event::poll(std::time::Duration::from_millis(16))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            // Clear status on any key
            self.status_message = None;

            match self.input_mode {
                InputMode::Replying | InputMode::Posting | InputMode::CrossPosting => {
                    self.handle_input_mode(key.code).await
                }
                InputMode::Normal => self.handle_normal_input(key.code).await,
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
                        InputMode::CrossPosting => self.send_cross_post().await,
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
            KeyCode::Char('P') => self.start_cross_post(), // Shift+P for cross-post
            KeyCode::Char('R') => self.refresh_threads().await,
            KeyCode::Tab | KeyCode::Char(']') => self.toggle_platform(),
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
        // Check if using multi-platform mode
        let has_selection = if let Some(state) = self.platform_states.get(&self.current_platform) {
            state.list_state.selected().is_some()
        } else {
            self.list_state.selected().is_some()
        };

        if has_selection {
            self.input_mode = InputMode::Replying;
            self.input_buffer.clear();
        }
    }

    fn start_post(&mut self) {
        self.input_mode = InputMode::Posting;
        self.input_buffer.clear();
    }

    fn start_cross_post(&mut self) {
        if self.clients.is_empty() {
            self.status_message = Some("No platforms available for cross-posting".to_string());
            return;
        }

        if self.clients.len() < 2 {
            self.status_message =
                Some("Cross-posting requires multiple platforms configured".to_string());
            return;
        }

        self.input_mode = InputMode::CrossPosting;
        self.input_buffer.clear();
    }

    async fn send_reply(&mut self) {
        let tx = self.event_tx.clone();
        let text = self.input_buffer.clone();

        // Check if using multi-platform mode
        if let Some(state) = self.platform_states.get(&self.current_platform) {
            // Get the post ID to reply to: selected reply or main post
            let reply_to_id = if let Some(reply_idx) = state.reply_selection {
                Self::get_platform_reply_id_at_index(&state.selected_replies, reply_idx)
            } else if let Some(idx) = state.list_state.selected() {
                state.posts.get(idx).map(|p| p.id.clone())
            } else {
                None
            };

            if let Some(post_id) = reply_to_id {
                if let Some(client) = self.clients.get(&self.current_platform) {
                    let client = client.clone();
                    let platform = self.current_platform;

                    info!("Sending reply to {} on {}", post_id, platform);
                    self.status_message = Some(format!("Replying on {}...", platform));

                    tokio::spawn(async move {
                        let result = client.reply_to_post(&post_id, &text).await;
                        let _ = tx
                            .send(AppEvent::PlatformReplyResult(
                                platform,
                                result.map(|_| ()).map_err(|e| e.to_string()),
                            ))
                            .await;
                    });
                }
            }
        } else {
            // Legacy Threads mode
            let reply_to_id = if let Some(reply_idx) = self.reply_selection {
                Self::get_reply_id_at_index(&self.selected_replies, reply_idx)
            } else if let Some(idx) = self.list_state.selected() {
                self.threads.get(idx).map(|t| t.id.clone())
            } else {
                None
            };

            if let Some(thread_id) = reply_to_id {
                let client = self.client.clone();

                info!("Sending reply to {}", thread_id);
                self.status_message = Some("Sending reply...".to_string());

                tokio::spawn(async move {
                    let result = client.reply_to_thread(&thread_id, &text).await;
                    let _ = tx
                        .send(AppEvent::ReplyResult(
                            result.map(|_| ()).map_err(|e| e.to_string()),
                        ))
                        .await;
                });
            }
        }
    }

    async fn send_post(&mut self) {
        let text = self.input_buffer.clone();
        info!("Sending new post to {}", self.current_platform);
        let tx = self.event_tx.clone();

        self.status_message = Some(format!("Posting to {}...", self.current_platform));

        // Check if we're using multi-platform mode
        if let Some(client) = self.clients.get(&self.current_platform) {
            let client = client.clone();
            let platform = self.current_platform;
            tokio::spawn(async move {
                let result = client.create_post(&text).await;
                let _ = tx
                    .send(AppEvent::PlatformPostResult(
                        platform,
                        result.map(|_| ()).map_err(|e| e.to_string()),
                    ))
                    .await;
            });
        } else {
            // Legacy Threads mode
            let client = self.client.clone();
            tokio::spawn(async move {
                let result = client.post_thread(&text).await;
                let _ = tx
                    .send(AppEvent::PostResult(
                        result.map(|_| ()).map_err(|e| e.to_string()),
                    ))
                    .await;
            });
        }
    }

    async fn send_cross_post(&mut self) {
        let text = self.input_buffer.clone();
        info!("Cross-posting to all platforms");

        let tx = self.event_tx.clone();
        let clients = self.clients.clone();

        if clients.is_empty() {
            self.status_message = Some("No platforms configured for cross-posting".to_string());
            return;
        }

        self.status_message = Some(format!("Cross-posting to {} platforms...", clients.len()));

        tokio::spawn(async move {
            for (platform, client) in clients.iter() {
                let result = client.create_post(&text).await;
                let _ = tx
                    .send(AppEvent::PlatformPostResult(
                        *platform,
                        result.map(|_| ()).map_err(|e| e.to_string()),
                    ))
                    .await;
            }
        });
    }

    async fn refresh_threads(&mut self) {
        debug!("Refreshing {}", self.current_platform);
        self.status_message = Some("Refreshing...".to_string());

        // If using multi-platform mode and current platform has a client, refresh it
        if let Some(client) = self.clients.get(&self.current_platform) {
            let client = client.clone();
            match client.get_posts(Some(25)).await {
                Ok(posts) => {
                    debug!("Refreshed: {} posts for {}", posts.len(), self.current_platform);
                    if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
                        state.posts = posts;
                        if state.list_state.selected().is_none() && !state.posts.is_empty() {
                            state.list_state.select(Some(0));
                        }
                    }
                    self.status_message = Some(format!("{} refreshed", self.current_platform));
                }
                Err(e) => {
                    error!("Refresh failed for {}: {}", self.current_platform, e);
                    self.status_message = Some(format!("Refresh failed: {}", e));
                }
            }
        } else {
            // Fall back to legacy Threads refresh
            match self.client.get_threads(Some(25)).await {
                Ok(resp) => {
                    debug!("Refreshed: {} threads", resp.data.len());
                    self.threads = resp.data;
                    self.status_message = Some("Refreshed".to_string());
                }
                Err(e) => {
                    error!("Refresh failed: {}", e);
                    self.status_message = Some(format!("Refresh failed: {}", e));
                }
            }
        }
    }

    fn maybe_load_replies(&mut self) {
        // Check if using multi-platform mode
        if let Some(state) = self.platform_states.get(&self.current_platform) {
            if let Some(idx) = state.list_state.selected() {
                if let Some(post) = state.posts.get(idx) {
                    // Check if we already loaded replies for this post
                    if state.loaded_replies_for.as_ref() == Some(&post.id) {
                        return;
                    }

                    let post_id = post.id.clone();
                    let tx = self.event_tx.clone();
                    let platform = self.current_platform;

                    // Get client for current platform
                    if let Some(client) = self.clients.get(&self.current_platform) {
                        let client = client.clone();

                        // Clear old replies in state
                        if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
                            state.selected_replies.clear();
                            state.loaded_replies_for = None;
                            state.reply_selection = None;
                        }

                        tokio::spawn(async move {
                            let result = client
                                .get_post_replies(&post_id, 2)
                                .await
                                .map(|replies| {
                                    // Convert platform::ReplyThread to PlatformReplyThread
                                    fn convert(replies: Vec<crate::platform::ReplyThread>) -> Vec<PlatformReplyThread> {
                                        replies.into_iter().map(|r| PlatformReplyThread {
                                            post: r.post,
                                            replies: convert(r.replies),
                                        }).collect()
                                    }
                                    convert(replies)
                                })
                                .map_err(|e| e.to_string());
                            let _ = tx.send(AppEvent::PlatformRepliesLoaded(platform, post_id, result)).await;
                        });
                    }
                }
            }
        } else {
            // Legacy Threads mode
            if let Some(idx) = self.list_state.selected()
                && let Some(thread) = self.threads.get(idx)
                && self.loaded_replies_for.as_ref() != Some(&thread.id)
            {
                let thread_id = thread.id.clone();
                let client = self.client.clone();
                let tx = self.event_tx.clone();

                // Clear old replies while loading
                self.selected_replies.clear();
                self.loaded_replies_for = None;
                self.reply_selection = None;

                tokio::spawn(async move {
                    let result = client
                        .get_thread_replies_nested(&thread_id, 2) // 2 levels deep
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(AppEvent::RepliesLoaded(thread_id, result)).await;
                });
            }
        }
    }

    fn move_down(&mut self) {
        match self.active_panel {
            Panel::Threads => {
                // Check if using multi-platform mode
                if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
                    if state.posts.is_empty() {
                        return;
                    }
                    let i = match state.list_state.selected() {
                        Some(i) => {
                            if i >= state.posts.len().saturating_sub(1) {
                                0
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    state.list_state.select(Some(i));
                } else {
                    // Legacy mode
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
            }
            Panel::Detail => self.reply_move_down(),
        }
    }

    fn move_up(&mut self) {
        match self.active_panel {
            Panel::Threads => {
                // Check if using multi-platform mode
                if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
                    if state.posts.is_empty() {
                        return;
                    }
                    let i = match state.list_state.selected() {
                        Some(i) => {
                            if i == 0 {
                                state.posts.len().saturating_sub(1)
                            } else {
                                i - 1
                            }
                        }
                        None => 0,
                    };
                    state.list_state.select(Some(i));
                } else {
                    // Legacy mode
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
            }
            Panel::Detail => self.reply_move_up(),
        }
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
        // Check if using multi-platform mode
        if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
            if state.reply_selection.is_some() {
                state.reply_selection = None;
            } else {
                self.active_panel = Panel::Threads;
            }
        } else {
            // Legacy mode: if a reply is selected, deselect it first
            if self.reply_selection.is_some() {
                self.reply_selection = None;
            } else {
                // Otherwise go back to threads panel
                self.active_panel = Panel::Threads;
            }
        }
    }

    /// Count total flattened replies (legacy)
    fn count_replies(replies: &[ReplyThread]) -> usize {
        replies
            .iter()
            .fold(0, |acc, r| acc + 1 + Self::count_replies(&r.replies))
    }

    /// Count total flattened replies (platform)
    fn count_platform_replies(replies: &[PlatformReplyThread]) -> usize {
        replies
            .iter()
            .fold(0, |acc, r| acc + 1 + Self::count_platform_replies(&r.replies))
    }

    /// Get the reply ID at the given flattened index (legacy)
    fn get_reply_id_at_index(replies: &[ReplyThread], target: usize) -> Option<String> {
        let mut current = 0;
        fn find(replies: &[ReplyThread], target: usize, current: &mut usize) -> Option<String> {
            for reply in replies {
                if *current == target {
                    return Some(reply.thread.id.clone());
                }
                *current += 1;
                if let Some(id) = find(&reply.replies, target, current) {
                    return Some(id);
                }
            }
            None
        }
        find(replies, target, &mut current)
    }

    /// Get the reply ID at the given flattened index (platform)
    fn get_platform_reply_id_at_index(replies: &[PlatformReplyThread], target: usize) -> Option<String> {
        let mut current = 0;
        fn find(replies: &[PlatformReplyThread], target: usize, current: &mut usize) -> Option<String> {
            for reply in replies {
                if *current == target {
                    return Some(reply.post.id.clone());
                }
                *current += 1;
                if let Some(id) = find(&reply.replies, target, current) {
                    return Some(id);
                }
            }
            None
        }
        find(replies, target, &mut current)
    }

    fn reply_move_down(&mut self) {
        // Check if using multi-platform mode
        if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
            let count = Self::count_platform_replies(&state.selected_replies);
            if count == 0 {
                return;
            }
            state.reply_selection = Some(match state.reply_selection {
                Some(i) if i >= count - 1 => 0,
                Some(i) => i + 1,
                None => 0,
            });
        } else {
            // Legacy mode
            let count = Self::count_replies(&self.selected_replies);
            if count == 0 {
                return;
            }
            self.reply_selection = Some(match self.reply_selection {
                Some(i) if i >= count - 1 => 0,
                Some(i) => i + 1,
                None => 0,
            });
        }
    }

    fn reply_move_up(&mut self) {
        // Check if using multi-platform mode
        if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
            let count = Self::count_platform_replies(&state.selected_replies);
            if count == 0 {
                return;
            }
            state.reply_selection = Some(match state.reply_selection {
                Some(0) | None => count.saturating_sub(1),
                Some(i) => i - 1,
            });
        } else {
            // Legacy mode
            let count = Self::count_replies(&self.selected_replies);
            if count == 0 {
                return;
            }
            self.reply_selection = Some(match self.reply_selection {
                Some(0) | None => count.saturating_sub(1),
                Some(i) => i - 1,
            });
        }
    }
}
