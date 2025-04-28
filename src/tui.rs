/*
TUI related components for the libp2p ping application.
*/

// TUI imports
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    symbols::border,
    text::{Line, Text, Span},
    widgets::{Block, Paragraph, Widget, List, ListItem},
    layout::{Constraint, Layout},
    style::{Color, Style},
};
use crossterm::event;
use std::path::PathBuf;
use std::collections::HashMap;
use std::time::Instant;
use std::time::Duration;

// libp2p imports
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, PeerId};
use crate::behavior::SwapBytesBehaviourEvent;

/// Time to wait before resetting the pinging state indicator.
pub const PINGING_DURATION: Duration = Duration::from_millis(2000);

/// Input modes for the TUI.
#[derive(Debug, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
}

/// Represents the currently focused UI pane.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum FocusPane {
    #[default]
    Console,
    Chat,
    UsersList,
}

/// Represents the online status of a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnlineStatus {
    Online,
    Offline,
}

/// Holds information about a discovered peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub nickname: Option<String>,
    pub status: OnlineStatus,
    pub last_seen: Instant,
}

/// Application state for the TUI.
#[derive(Debug)]
pub struct App {
    /// Log history (limited).
    pub log: Vec<String>,
    /// Current value of the input box.
    pub input: String,
    /// Position of cursor in the input box.
    pub cursor_position: usize,
    /// Current input mode.
    pub input_mode: InputMode,
    /// Flag indicating if the application should exit.
    pub exit: bool,
    /// Currently focused pane.
    pub focused_pane: FocusPane,
    /// Vertical scroll position for the console log.
    pub console_scroll: usize,
    /// Height of the console viewport (number of visible lines).
    pub console_viewport_height: usize,
    /// Addresses the Swarm is listening on.
    pub listening_addresses: Vec<Multiaddr>,
    /// Currently configured download directory (must be verified).
    pub download_dir: Option<PathBuf>,
    /// User's chosen nickname (must be verified).
    pub nickname: Option<String>,
    /// The local peer's unique ID.
    pub local_peer_id: Option<PeerId>,
    /// Map of discovered peers and their info.
    pub peers: HashMap<PeerId, PeerInfo>,
    /// Flag indicating if a ping command is currently active.
    pub pinging: bool,
    /// Timestamp when the current ping command was initiated.
    pub ping_start_time: Option<Instant>,
}

impl Default for App {
    fn default() -> Self {
        App {
            log: Vec::new(),
            input: String::new(),
            cursor_position: 0,
            input_mode: InputMode::default(),
            exit: false,
            focused_pane: FocusPane::default(),
            console_scroll: 0, // Start at the top
            console_viewport_height: 2, // default minimal height
            listening_addresses: Vec::new(), // Initialize empty list
            download_dir: None, // Initialize as None
            nickname: None, // Initialize nickname as None
            local_peer_id: None, // Initialize PeerId as None
            peers: HashMap::new(), // Initialize empty peers map
            pinging: false, // Initialize pinging state
            ping_start_time: None, // Initialize ping start time
        }
    }
}

// Max number of lines to keep in the log history
const MAX_LOG_LINES: usize = 1000;

impl App {
    /// Adds a new message line to the log, maintaining a maximum history size
    /// and auto-scrolling to the bottom.
    pub fn push<S: Into<String>>(&mut self, line: S) {
        self.log.push(line.into());
        if self.log.len() > MAX_LOG_LINES {
            self.log.drain(0..self.log.len() - MAX_LOG_LINES);
            // Adjust scroll if necessary when lines are removed from the top
            // This logic might not be strictly needed if we always scroll down,
            // but it doesn't hurt.
            let max_scroll = self.log.len().saturating_sub(1);
            self.console_scroll = self.console_scroll.min(max_scroll);
        }
        // Auto-scroll to the bottom respecting viewport height
        let viewport = self.console_viewport_height.max(1);
        let new_scroll_pos = self.log.len().saturating_sub(viewport);
        self.console_scroll = new_scroll_pos;
    }

    /// Adds a new message line to the log (prepended with "[LOG]"),
    /// maintaining a maximum history size and auto-scrolling to the bottom.
    pub fn log<S: Into<String>>(&mut self, line: S) {
        self.log.push(format!("[LOG] {}", line.into()));
        if self.log.len() > MAX_LOG_LINES {
            self.log.drain(0..self.log.len() - MAX_LOG_LINES);
            // Adjust scroll if necessary (as above)
            let max_scroll = self.log.len().saturating_sub(1);
            self.console_scroll = self.console_scroll.min(max_scroll);
        }
        // Auto-scroll to the bottom respecting viewport height
        let viewport = self.console_viewport_height.max(1);
        let new_scroll_pos = self.log.len().saturating_sub(viewport);
        self.console_scroll = new_scroll_pos;
    }

    // --- Input Handling Methods (adapted from input_example.rs) ---

    /// Moves the cursor one character to the left.
    pub fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.cursor_position.saturating_sub(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_left);
    }

    /// Moves the cursor one character to the right.
    pub fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.cursor_position.saturating_add(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_right);
    }

    /// Inserts a character at the current cursor position.
    pub fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.input.insert(index, new_char);
        self.move_cursor_right();
    }

    /// Deletes the character before the current cursor position.
    pub fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.cursor_position != 0;
        if is_not_cursor_leftmost {
            let current_index = self.cursor_position;
            let from_left_to_current_index = current_index - 1;
            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.input.chars().skip(current_index);
            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    /// Returns the byte index based on the character position for UTF-8 strings.
    fn byte_index(&self) -> usize {
        self.input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.cursor_position)
            .unwrap_or(self.input.len())
    }

    /// Clamps the cursor position within the bounds of the input string's characters.
    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    /// Resets the cursor position to the beginning of the input string.
    pub fn reset_cursor(&mut self) {
        self.cursor_position = 0;
    }

    /// Submits the current input as a command.
    /// Returns an optional AppEvent if a command needs to be processed by the main loop.
    pub fn submit_command(&mut self) -> Option<AppEvent> {
        self.push(format!("> {}", self.input)); // Log the entered command

        // Create a copy of the input string to avoid borrow checker issues
        let input_copy = self.input.clone(); 
        let command_input = input_copy.strip_prefix('/').unwrap_or(&input_copy);

        // Call the command processor from the commands module
        let event_to_send = crate::commands::process_command(command_input, self);

        // Clear the original input field
        self.input.clear();
        self.reset_cursor();
        self.input_mode = InputMode::Normal; // Return to normal mode after submit

        event_to_send // Return the event for the main loop
    }

    // --- Rendering Helper Functions ---

    /// Renders the console pane (log and input).
    fn render_console_pane(&self, area: Rect, buf: &mut Buffer) {
        let focused_style = Style::default().fg(Color::Yellow);
        let unfocused_style = Style::default();

        let console_title_bottom = match self.input_mode {
            InputMode::Normal => " Focus: Tab | Scroll: ↑/↓ | Quit: Ctrl+Q ".bold(),
            InputMode::Editing => " Submit: Enter | Cancel: Esc ".bold(),
        };
        let console_block = Block::bordered()
            .title(" Console ".bold())
            .title_bottom(Line::from(console_title_bottom))
            .border_set(border::THICK)
            .border_style(if self.focused_pane == FocusPane::Console { focused_style } else { unfocused_style });

        // Layout within the console block (Log + Input)
        let console_inner_area = console_block.inner(area);
        let console_chunks = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(console_inner_area);

        let log_area = console_chunks[0];
        let input_area = console_chunks[1];

        // Render the console block border first
        console_block.render(area, buf);

        // Render Log Panel within its area
        let log_text = Text::from(
            self.log
                .iter()
                .map(|l| Line::from(l.clone()))
                .collect::<Vec<_>>(),
        );
        let log_paragraph = Paragraph::new(log_text)
            .scroll((self.console_scroll as u16, 0));
        log_paragraph.render(log_area, buf);

        // Render Input Box within its area
        let input_title = if self.pinging {
            " Input (Pinging...) " // Indicate pinging state
        } else {
            " Input (/) " // Normal state
        };
        let input_paragraph = Paragraph::new(self.input.as_str())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(Color::Yellow),
            })
            .block(Block::bordered().title(input_title.bold())); // Use dynamic title
        input_paragraph.render(input_area, buf);
    }

    /// Renders the users list pane.
    fn render_users_pane(&self, area: Rect, buf: &mut Buffer) {
        let focused_style = Style::default().fg(Color::Yellow);
        let unfocused_style = Style::default();
        let is_focused = self.focused_pane == FocusPane::UsersList;

        let users_block = Block::bordered()
            .title(" Users ".bold())
            .border_set(border::THICK)
            .border_style(if is_focused { focused_style } else { unfocused_style });

        let inner_area = users_block.inner(area);
        users_block.render(area, buf); // Render block border first

        // --- Render actual user list inside inner_area ---
        let mut items = Vec::new();
        // Sort peers by PeerId (base58 representation) for a consistent order.
        let mut sorted_peers: Vec<_> = self.peers.iter().collect();
        sorted_peers.sort_by_key(|(id, _)| id.to_base58());

        for (peer_id, peer_info) in sorted_peers {
            // Determine the display name: Use the nickname if available,
            // otherwise show "No-Name" followed by the last 6 chars of the PeerId.
            // Nicknames will be populated later via Gossipsub messages.
            let display_name = peer_info.nickname.clone().unwrap_or_else(|| {
                let id_str = peer_id.to_base58();
                let len = id_str.len();
                // Ensure we don't panic if the id_str is unexpectedly short
                let start_index = len.saturating_sub(6); // Get index 6 chars from the end, or 0 if too short
                format!("No-Name (...{})", &id_str[start_index..])
            });

            // Style the status prefix based on whether the peer is Online or Offline.
            let status_style = match peer_info.status {
                OnlineStatus::Online => Style::default().fg(Color::Green),
                OnlineStatus::Offline => Style::default().fg(Color::Gray),
            };

            // Define the status prefix string.
            let prefix = match peer_info.status {
                OnlineStatus::Online => "[✓] ",
                OnlineStatus::Offline => "[✗] ",
            };
            // Construct the line with styled prefix and raw display name using Spans
            // for explicit control over styling.
            let line = Line::from(vec![
                Span::styled(prefix, status_style), // Use Span::styled
                Span::raw(display_name),       // Use Span::raw for the string part
            ]);
            items.push(ListItem::new(line));
        }

        // Create the list widget with the generated items.
        // TODO: Implement scroll state handling if the list becomes long.
        let users_list = List::new(items);
        // Render the list within the inner area
        users_list.render(inner_area, buf);
    }

    /// Renders the chat pane.
    fn render_chat_pane(&self, area: Rect, buf: &mut Buffer) {
        let focused_style = Style::default().fg(Color::Yellow);
        let unfocused_style = Style::default();

        let chat_title = " Global Chat ".bold();
        let chat_block = Block::bordered()
            .title(chat_title)
            .border_set(border::THICK)
            .border_style(if self.focused_pane == FocusPane::Chat { focused_style } else { unfocused_style });

        let inner_area = chat_block.inner(area);
        chat_block.render(area, buf);

        // TODO: Render chat messages inside inner_area
        let placeholder_text = Paragraph::new("Chat messages coming soon...");
        placeholder_text.render(inner_area, buf);
    }
}

/// Implements the rendering logic for the `App` state using Ratatui.
impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Compute layout areas
        let (chat_area, console_area, users_area) = layout_chunks(area);

        // --- Render Panes using Helper Functions ---
        self.render_chat_pane(chat_area, buf);
        self.render_console_pane(console_area, buf);
        self.render_users_pane(users_area, buf);

        // Note: Cursor setting logic remains in main.rs as it requires the `Frame` (`f`).
    }
}

/// Events that drive the application's state changes.
#[derive(Debug)]
pub enum AppEvent {
    /// User keyboard input.
    Input(event::KeyEvent),
    /// Event originating from the libp2p Swarm.
    Swarm(SwarmEvent<SwapBytesBehaviourEvent>),
    /// User command to dial a peer (sent from UI to Swarm task).
    Dial(Multiaddr),
    /// Message to be logged in the UI (sent from Swarm task to UI).
    LogMessage(String),
    /// A peer was discovered via mDNS.
    PeerDiscovered(PeerId),
    /// An mDNS record for a peer expired.
    PeerExpired(PeerId),
    /// User command to quit the application.
    Quit,
}

// Computes the layout rectangles for the chat, console, and users list.
pub fn layout_chunks(area: Rect) -> (Rect, Rect, Rect) {
    let main_chunks = Layout::horizontal([
        Constraint::Percentage(75),
        Constraint::Percentage(25),
    ])
    .split(area);
    let left_area = main_chunks[0];
    let users_area = main_chunks[1];

    let left_chunks = Layout::vertical([
        Constraint::Percentage(67),
        Constraint::Percentage(33),
    ])
    .split(left_area);
    let chat_area = left_chunks[0];
    let console_area = left_chunks[1];
    (chat_area, console_area, users_area)
}
