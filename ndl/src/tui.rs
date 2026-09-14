use crate::platform::{Platform, Post, ReplyThread, SocialClient};
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::collections::HashMap;
use std::io::{self, stdout};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Width of each engagement-count column in the posts list
const STAT_COL: usize = 4;
/// Count column legend: likes, replies, reposts, quotes
const STAT_SYMBOLS: [&str; 4] = ["♥", "💬", "↻", "❝"];
/// Appended as a fifth column when a platform reports shares / sends
const SHARE_SYMBOL: &str = "➤";
/// How often the follower count is re-fetched per platform
const FOLLOWERS_INTERVAL: std::time::Duration = std::time::Duration::from_secs(300);

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
    PostsUpdated(Platform, Vec<Post>),
    ReplyResult(Platform, Result<(), String>),
    PostResult(Platform, Result<(), String>),
    LikeResult(Platform, String, Result<(), String>),
    RepliesLoaded(Platform, String, Result<Vec<ReplyThread>, String>),
    FollowersUpdated(Platform, u64),
}

/// Platform-specific state
pub struct PlatformState {
    pub posts: Vec<Post>,
    pub list_state: ListState,
    pub selected_replies: Vec<ReplyThread>,
    pub loaded_replies_for: Option<String>,
    pub reply_selection: Option<usize>,
    /// Latest follower count, once the platform has reported one
    pub followers: Option<u64>,
    /// The first count seen this session, so the title can show the session's drift
    pub followers_baseline: Option<u64>,
}

impl PlatformState {
    fn new() -> Self {
        Self {
            posts: Vec::new(),
            list_state: ListState::default(),
            selected_replies: Vec::new(),
            loaded_replies_for: None,
            reply_selection: None,
            followers: None,
            followers_baseline: None,
        }
    }

    /// Flag a post (in the list or the loaded replies) as liked by the user
    fn mark_liked(&mut self, post_id: &str) {
        for post in &mut self.posts {
            if post.id == post_id {
                post.liked = true;
            }
        }
        fn mark(replies: &mut [ReplyThread], post_id: &str) {
            for reply in replies {
                if reply.post.id == post_id {
                    reply.post.liked = true;
                }
                mark(&mut reply.replies, post_id);
            }
        }
        mark(&mut self.selected_replies, post_id);
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
    pub current_platform: Platform,
    pub clients: HashMap<Platform, Arc<Box<dyn SocialClient>>>,
    pub platform_states: HashMap<Platform, PlatformState>,
}

impl App {
    pub fn new(clients: HashMap<Platform, Box<dyn SocialClient>>) -> Self {
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
        }
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

        self.status_message = None;
    }

    fn start_refresh_task(&self) {
        for (platform, client) in &self.clients {
            let platform = *platform;
            let client = client.clone();
            let tx = self.event_tx.clone();

            tokio::spawn(async move {
                loop {
                    // this goes to 11
                    tokio::time::sleep(std::time::Duration::from_secs(11)).await;

                    if let Ok(posts) = client.get_posts(Some(25)).await {
                        let _ = tx.send(AppEvent::PostsUpdated(platform, posts)).await;
                    }
                }
            });
        }

        // Follower counts move slowly, and Threads charges an API call for one
        for (platform, client) in &self.clients {
            let platform = *platform;
            let client = client.clone();
            let tx = self.event_tx.clone();

            tokio::spawn(async move {
                loop {
                    match client.get_follower_count().await {
                        Ok(Some(followers)) => {
                            let _ = tx
                                .send(AppEvent::FollowersUpdated(platform, followers))
                                .await;
                        }
                        Ok(None) => debug!("No follower count available for {}", platform),
                        Err(e) => debug!("Follower count failed for {}: {}", platform, e),
                    }

                    tokio::time::sleep(FOLLOWERS_INTERVAL).await;
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
            .unwrap_or("? for help | p to post | r to reply | i to like | o to open | R to refresh")
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
i            Like thread or reply
o            Open thread or reply in browser
↻ / ♥        Reposted / liked by you
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

        let Some(state) = self.platform_states.get(&self.current_platform) else {
            return;
        };

        // Count columns appear only when the platform supplies counts with its posts
        let show_stats = state.posts.iter().any(|p| p.stats.is_some());
        let show_shares = state
            .posts
            .iter()
            .any(|p| p.stats.is_some_and(|s| s.shares.is_some()));
        // Inner width minus the "> " highlight gutter
        let item_w = area.width.saturating_sub(4) as usize;
        let stats_w = if show_stats {
            STAT_COL * (STAT_SYMBOLS.len() + usize::from(show_shares))
        } else {
            0
        };
        let text_w = item_w.saturating_sub(stats_w).max(8);

        let items: Vec<ListItem> = state
            .posts
            .iter()
            .map(|p| {
                let display = if let Some(text) = p.text.as_deref() {
                    text.replace(['\n', '\r'], " ")
                } else {
                    // No text - show media type indicator
                    match p.media_type.as_deref() {
                        Some("REPOST_FACADE") => "[repost]".to_string(),
                        Some("IMAGE") => "[image]".to_string(),
                        Some("VIDEO") => "[video]".to_string(),
                        Some("CAROUSEL_ALBUM") => "[carousel]".to_string(),
                        Some(other) => format!("[{}]", other.to_lowercase()),
                        None => "[no text]".to_string(),
                    }
                };
                let display = if p.reposted {
                    format!("↻ {}", display)
                } else {
                    display
                };
                let mut spans = vec![Span::raw(Self::fit_width(&display, text_w))];
                match (p.stats, show_stats) {
                    (Some(stats), _) => {
                        // Your own like is counted in red
                        let like_style = if p.liked {
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        };
                        spans.push(Span::styled(
                            Self::pad_left(&Self::compact(stats.likes), STAT_COL),
                            like_style,
                        ));
                        spans.push(Span::raw(Self::pad_left(
                            &Self::compact(stats.replies),
                            STAT_COL,
                        )));
                        spans.push(Span::raw(Self::pad_left(
                            &Self::compact(stats.reposts),
                            STAT_COL,
                        )));
                        spans.push(Span::raw(Self::pad_left(
                            &Self::compact(stats.quotes),
                            STAT_COL,
                        )));
                        if show_shares {
                            let shares = stats.shares.map_or("-".to_string(), Self::compact);
                            spans.push(Span::raw(Self::pad_left(&shares, STAT_COL)));
                        }
                    }
                    // Platform has counts but not for this post: leave the columns blank
                    (None, true) => {}
                    (None, false) => {
                        if p.liked {
                            spans.push(Span::styled(" ♥", Style::default().fg(Color::Red)));
                        }
                    }
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let mut title = format!(" {} ({})", self.current_platform, state.posts.len());
        if let Some(followers) = state.followers {
            title.push_str(&format!(" · 👥 {}", followers));
            // Drift since the first count seen this session
            if let Some(baseline) = state.followers_baseline {
                let delta = followers as i64 - baseline as i64;
                if delta != 0 {
                    title.push_str(&format!(" ({:+})", delta));
                }
            }
        }
        title.push(' ');
        let mut block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);
        if show_stats {
            // Legend sits in the top border, right-aligned over the count columns
            let mut legend: String = STAT_SYMBOLS
                .iter()
                .map(|s| Self::pad_left(s, STAT_COL))
                .collect();
            if show_shares {
                legend.push_str(&Self::pad_left(SHARE_SYMBOL, STAT_COL));
            }
            block = block.title_top(Line::from(legend).right_aligned());
        }
        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
            frame.render_stateful_widget(list, area, &mut state.list_state);
        }
    }

    fn draw_detail(&self, frame: &mut Frame, area: Rect) {
        let is_active = self.active_panel == Panel::Detail;
        let border_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let content = if let Some(state) = self.platform_states.get(&self.current_platform) {
            if let Some(idx) = state.list_state.selected() {
                if let Some(post) = state.posts.get(idx) {
                    let author = post.author_handle.as_deref().unwrap_or("unknown");
                    let timestamp = post.timestamp.as_deref().unwrap_or("");
                    let text = if let Some(t) = post.text.as_deref() {
                        t.to_string()
                    } else {
                        // No text - show media type indicator with permalink
                        match post.media_type.as_deref() {
                            Some("REPOST_FACADE") => {
                                let link = post.permalink.as_deref().unwrap_or("");
                                format!("[Repost]\n{}", link)
                            }
                            Some("IMAGE") => "[Image post]".to_string(),
                            Some("VIDEO") => "[Video post]".to_string(),
                            Some("CAROUSEL_ALBUM") => "[Carousel post]".to_string(),
                            Some("QUOTE") => "[Quote post]".to_string(),
                            Some("LINK") => "[Link post]".to_string(),
                            Some(other) => format!("[{} post]", other),
                            None => "[No text]".to_string(),
                        }
                    };

                    // Handle on the left, timestamp flush right, inside the borders
                    let inner_width = area.width.saturating_sub(2) as usize;
                    let mut handle = format!("@{}", author);
                    if post.reposted {
                        handle.push_str(" ↻");
                    }
                    if post.liked {
                        handle.push_str(" ♥");
                    }
                    let header = Self::spread_line(&handle, timestamp, inner_width);
                    let mut content = match post.stats {
                        Some(stats) => {
                            let mut counts = format!(
                                "♥ {}  💬 {}  ↻ {}  ❝ {}",
                                stats.likes, stats.replies, stats.reposts, stats.quotes
                            );
                            if let Some(shares) = stats.shares {
                                counts.push_str(&format!("  ➤ {}", shares));
                            }
                            format!("{}\n{}\n\n{}", header, counts, text)
                        }
                        None => format!("{}\n\n{}", header, text),
                    };

                    // Add replies section
                    if !state.selected_replies.is_empty() {
                        content.push_str(
                            "\n\n--- Replies (j/k to select, r to reply, i to like) ---\n",
                        );
                        let selected_idx = state.reply_selection;
                        fn format_replies(
                            replies: &[ReplyThread],
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
                                let heart = if reply.post.liked { " ♥" } else { "" };
                                out.push_str(&format!(
                                    "\n{}{}@{}: {}{}\n",
                                    marker, prefix, user, text, heart
                                ));
                                *counter += 1;
                                if !reply.replies.is_empty() {
                                    format_replies(
                                        &reply.replies,
                                        indent + 1,
                                        out,
                                        counter,
                                        selected,
                                    );
                                }
                            }
                        }
                        let mut counter = 0;
                        format_replies(
                            &state.selected_replies,
                            0,
                            &mut content,
                            &mut counter,
                            selected_idx,
                        );
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
        } else {
            "No post selected".to_string()
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
                AppEvent::PostResult(platform, result) => match result {
                    Ok(()) => {
                        info!("Post sent successfully to {}", platform);
                        self.status_message = Some(format!("Posted to {}!", platform));
                    }
                    Err(ref e) => {
                        error!("Post to {} failed: {}", platform, e);
                        self.status_message = Some(format!("{} error: {}", platform, e));
                    }
                },
                AppEvent::ReplyResult(platform, result) => match result {
                    Ok(()) => {
                        info!("Reply sent successfully to {}", platform);
                        self.status_message = Some(format!("Replied on {}!", platform));
                    }
                    Err(ref e) => {
                        error!("Reply to {} failed: {}", platform, e);
                        self.status_message = Some(format!("{} error: {}", platform, e));
                    }
                },
                AppEvent::LikeResult(platform, post_id, result) => match result {
                    Ok(()) => {
                        info!("Like sent successfully to {}", platform);
                        if let Some(state) = self.platform_states.get_mut(&platform) {
                            state.mark_liked(&post_id);
                        }
                        self.status_message = Some(format!("Liked on {}!", platform));
                    }
                    Err(ref e) => {
                        error!("Like on {} failed: {}", platform, e);
                        self.status_message = Some(format!("{} error: {}", platform, e));
                    }
                },
                AppEvent::FollowersUpdated(platform, followers) => {
                    debug!("{} follower count: {}", platform, followers);
                    if let Some(state) = self.platform_states.get_mut(&platform) {
                        state.followers = Some(followers);
                        if state.followers_baseline.is_none() {
                            state.followers_baseline = Some(followers);
                        }
                    }
                }
                AppEvent::RepliesLoaded(platform, post_id, result) => {
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
            KeyCode::Char('i') => self.like_selected(),
            KeyCode::Char('o') => self.open_selected(),
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
        let has_selection = self
            .platform_states
            .get(&self.current_platform)
            .is_some_and(|state| state.list_state.selected().is_some());

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

    /// The post an action should target: the selected reply, else the highlighted post
    fn selected_target(&self) -> Option<&Post> {
        let state = self.platform_states.get(&self.current_platform)?;
        if let Some(reply_idx) = state.reply_selection {
            Self::get_reply_at_index(&state.selected_replies, reply_idx)
        } else {
            let idx = state.list_state.selected()?;
            state.posts.get(idx)
        }
    }

    /// Lay `left` and `right` out on one line with `right` flush against `width`
    fn spread_line(left: &str, right: &str, width: usize) -> String {
        let used = left.width() + right.width();
        let pad = width.saturating_sub(used).max(1);
        format!("{}{}{}", left, " ".repeat(pad), right)
    }

    /// Shorten a count to fit a narrow column: `9999`, `12k`, `1.2M`
    fn compact(n: u64) -> String {
        match n {
            0..10_000 => n.to_string(),
            10_000..1_000_000 => format!("{}k", n / 1_000),
            _ => format!("{:.1}M", n as f64 / 1_000_000.0),
        }
    }

    /// Right-align `s` within `width` display cells
    fn pad_left(s: &str, width: usize) -> String {
        format!("{}{}", " ".repeat(width.saturating_sub(s.width())), s)
    }

    /// Truncate `s` to `width` display cells (ending in an ellipsis), then pad to exactly `width`
    fn fit_width(s: &str, width: usize) -> String {
        if s.width() <= width {
            return format!("{}{}", s, " ".repeat(width - s.width()));
        }
        let target = width.saturating_sub(1); // leave room for the ellipsis
        let mut out = String::new();
        let mut used = 0;
        for ch in s.chars() {
            let w = ch.width().unwrap_or(0);
            if used + w > target {
                break;
            }
            out.push(ch);
            used += w;
        }
        out.push('…');
        used += 1;
        format!("{}{}", out, " ".repeat(width.saturating_sub(used)))
    }

    fn like_selected(&mut self) {
        let Some(target) = self.selected_target() else {
            return;
        };
        if target.liked {
            self.status_message = Some(format!("Already liked on {}", self.current_platform));
            return;
        }
        let post_id = target.id.clone();
        let Some(client) = self.clients.get(&self.current_platform) else {
            return;
        };

        let client = client.clone();
        let tx = self.event_tx.clone();
        let platform = self.current_platform;

        info!("Liking {} on {}", post_id, platform);
        self.status_message = Some(format!("Liking on {}...", platform));

        tokio::spawn(async move {
            let result = client.like_post(&post_id).await;
            let _ = tx
                .send(AppEvent::LikeResult(
                    platform,
                    post_id,
                    result.map_err(|e| e.to_string()),
                ))
                .await;
        });
    }

    /// Open the highlighted post (or selected reply) in the system browser
    fn open_selected(&mut self) {
        let Some(target) = self.selected_target() else {
            return;
        };
        let Some(url) = target.permalink.clone() else {
            self.status_message = Some("No link for this post".to_string());
            return;
        };
        match open::that_detached(&url) {
            Ok(()) => {
                info!("Opened {} in browser", url);
                self.status_message = Some(format!("Opened {}", url));
            }
            Err(e) => {
                error!("Failed to open {}: {}", url, e);
                self.status_message = Some(format!("Couldn't open browser: {}", e));
            }
        }
    }

    async fn send_reply(&mut self) {
        let tx = self.event_tx.clone();
        let text = self.input_buffer.clone();

        if let Some(post_id) = self.selected_target().map(|p| p.id.clone())
            && let Some(client) = self.clients.get(&self.current_platform)
        {
            let client = client.clone();
            let platform = self.current_platform;

            info!("Sending reply to {} on {}", post_id, platform);
            self.status_message = Some(format!("Replying on {}...", platform));

            tokio::spawn(async move {
                let result = client.reply_to_post(&post_id, &text).await;
                let _ = tx
                    .send(AppEvent::ReplyResult(
                        platform,
                        result.map_err(|e| e.to_string()),
                    ))
                    .await;
            });
        }
    }

    async fn send_post(&mut self) {
        let text = self.input_buffer.clone();
        info!("Sending new post to {}", self.current_platform);
        let tx = self.event_tx.clone();

        self.status_message = Some(format!("Posting to {}...", self.current_platform));

        let Some(client) = self.clients.get(&self.current_platform) else {
            self.status_message = Some("No client available".to_string());
            return;
        };

        let client = client.clone();
        let platform = self.current_platform;
        tokio::spawn(async move {
            let result = client.create_post(&text).await;
            let _ = tx
                .send(AppEvent::PostResult(
                    platform,
                    result.map_err(|e| e.to_string()),
                ))
                .await;
        });
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
                    .send(AppEvent::PostResult(
                        *platform,
                        result.map_err(|e| e.to_string()),
                    ))
                    .await;
            }
        });
    }

    async fn refresh_threads(&mut self) {
        debug!("Refreshing {}", self.current_platform);
        self.status_message = Some("Refreshing...".to_string());

        let Some(client) = self.clients.get(&self.current_platform) else {
            self.status_message = Some("No client available".to_string());
            return;
        };

        let client = client.clone();
        match client.get_posts(Some(25)).await {
            Ok(posts) => {
                debug!(
                    "Refreshed: {} posts for {}",
                    posts.len(),
                    self.current_platform
                );
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
    }

    fn maybe_load_replies(&mut self) {
        let Some(state) = self.platform_states.get(&self.current_platform) else {
            return;
        };

        let Some(idx) = state.list_state.selected() else {
            return;
        };

        let Some(post) = state.posts.get(idx) else {
            return;
        };

        // Check if we already loaded replies for this post
        if state.loaded_replies_for.as_ref() == Some(&post.id) {
            return;
        }

        let Some(client) = self.clients.get(&self.current_platform) else {
            return;
        };

        let post_id = post.id.clone();
        let tx = self.event_tx.clone();
        let platform = self.current_platform;
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
                .map_err(|e| e.to_string());
            let _ = tx
                .send(AppEvent::RepliesLoaded(platform, post_id, result))
                .await;
        });
    }

    fn move_down(&mut self) {
        match self.active_panel {
            Panel::Threads => {
                let Some(state) = self.platform_states.get_mut(&self.current_platform) else {
                    return;
                };
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
            }
            Panel::Detail => self.reply_move_down(),
        }
    }

    fn move_up(&mut self) {
        match self.active_panel {
            Panel::Threads => {
                let Some(state) = self.platform_states.get_mut(&self.current_platform) else {
                    return;
                };
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
        if let Some(state) = self.platform_states.get_mut(&self.current_platform) {
            if state.reply_selection.is_some() {
                state.reply_selection = None;
            } else {
                self.active_panel = Panel::Threads;
            }
        } else {
            self.active_panel = Panel::Threads;
        }
    }

    /// Count total flattened replies
    fn count_replies(replies: &[ReplyThread]) -> usize {
        replies
            .iter()
            .fold(0, |acc, r| acc + 1 + Self::count_replies(&r.replies))
    }

    /// Get the reply at the given flattened index
    fn get_reply_at_index(replies: &[ReplyThread], target: usize) -> Option<&Post> {
        let mut current = 0;
        fn find<'a>(
            replies: &'a [ReplyThread],
            target: usize,
            current: &mut usize,
        ) -> Option<&'a Post> {
            for reply in replies {
                if *current == target {
                    return Some(&reply.post);
                }
                *current += 1;
                if let Some(post) = find(&reply.replies, target, current) {
                    return Some(post);
                }
            }
            None
        }
        find(replies, target, &mut current)
    }

    fn reply_move_down(&mut self) {
        let Some(state) = self.platform_states.get_mut(&self.current_platform) else {
            return;
        };
        let count = Self::count_replies(&state.selected_replies);
        if count == 0 {
            return;
        }
        state.reply_selection = Some(match state.reply_selection {
            Some(i) if i >= count - 1 => 0,
            Some(i) => i + 1,
            None => 0,
        });
    }

    fn reply_move_up(&mut self) {
        let Some(state) = self.platform_states.get_mut(&self.current_platform) else {
            return;
        };
        let count = Self::count_replies(&state.selected_replies);
        if count == 0 {
            return;
        }
        state.reply_selection = Some(match state.reply_selection {
            Some(0) | None => count.saturating_sub(1),
            Some(i) => i - 1,
        });
    }
}
