/*
Code related to the Terminal User Interface (TUI)
*/

// Import necessary TUI components from `ratatui`
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
// Import terminal event handling from `crossterm`
use crossterm::event;
// Standard library imports for file paths, data structures, time, and random numbers
use std::path::PathBuf;
use std::collections::HashMap;
use std::time::{Instant, Duration};
use rand::Rng;

// Import necessary libp2p types for network interaction
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, PeerId};
// Import our custom network behavior events
use crate::behavior::SwapBytesBehaviourEvent;
// Import tokio filesystem for file handling in DownloadState
use tokio::fs::File as TokioFile;

/// Holds the details of a single chat message to be displayed.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// The unique ID of the peer who sent the message.
    pub sender_id: PeerId,
    /// The chosen nickname of the sender, if known.
    pub sender_nickname: Option<String>,
    /// The actual text content of the message.
    pub content: String,
    /// When the message was received (as milliseconds since epoch).
    pub timestamp_ms: u64,
}

/// Represents an item displayed within a private chat history.
#[derive(Debug, Clone)]
pub enum PrivateChatItem {
    /// A regular text message.
    Message(ChatMessage),
    /// A file offer received from the peer.
    Offer(PendingOfferDetails), // Reuse the PendingOfferDetails struct
    /// A file offer initiated by the local user.
    OfferSent(PendingOfferDetails),
    /// A file offer declined by the local user.
    OfferDeclined(PendingOfferDetails),
    /// A file offer sent by the local user that was declined by the remote peer.
    RemoteOfferDeclined(PendingOfferDetails),
    /// A file offer that was accepted by the local user.
    OfferAccepted(PendingOfferDetails),
    /// A file offer sent by the local user that was accepted by the remote peer.
    RemoteOfferAccepted(PendingOfferDetails),
    /// An ongoing file transfer's progress.
    TransferProgress {
        filename: String,
        received: u64,
        total: u64,
        // We might add start_time later for speed calculation
    },
    /// A successfully completed file transfer (download).
    TransferComplete {
        filename: String,
        final_path: PathBuf,
        size: u64,
    },
    /// A file transfer that failed.
    TransferFailed {
        filename: String,
        error: String,
    },
}

/// How long the "Pinging..." indicator stays visible after sending a ping.
pub const PINGING_DURATION: Duration = Duration::from_millis(2000);

/// Different modes the user can be in when interacting with the input boxes.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum InputMode {
    /// Default mode: Not actively typing in any input box. Keys control focus/scrolling.
    #[default]
    Normal,
    /// Typing a command in the console input box (e.g., `/dial ...`).
    Command,
    /// Typing a message in the chat input box.
    Chat,
}

/// Represents which main section (pane) of the UI currently has focus.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum FocusPane {
    /// The console area (logs and command input) is focused.
    #[default]
    Console,
    /// The chat area (messages and chat input) is focused.
    Chat,
    /// The list of users is focused.
    UsersList,
}

/// Tracks whether the user is currently viewing the global chat or a private chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatContext {
    /// Viewing the main public chat room.
    Global,
    /// Viewing a private one-on-one chat with a specific peer.
    Private {
        target_peer_id: PeerId, // The ID of the peer we're chatting with.
        target_nickname: Option<String>, // Their nickname, if we know it.
    },
}

/// Indicates whether a peer is currently considered online or offline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnlineStatus {
    Online,
    Offline,
}

/// Stores information about a peer we've discovered on the network.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// The peer's chosen nickname, if they've shared it.
    pub nickname: Option<String>,
    /// Whether the peer is currently online or offline.
    pub status: OnlineStatus,
    /// When we last heard from this peer (used for determining offline status).
    pub last_seen: Instant,
}

/// Stores the details of a file offer that is waiting for acceptance/rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOfferDetails {
    pub filename: String,
    pub size_bytes: u64,
    pub path: PathBuf,
}

/// Represents the state of an ongoing file download.
#[derive(Debug)] // Deriving Debug for App's Debug implementation
pub struct DownloadState {
    /// The local path where the file is being saved (likely temporary initially).
    pub local_path: PathBuf,
    /// The total expected size of the file in bytes.
    pub total_size: u64,
    /// How many bytes have been received so far.
    pub received: u64,
    /// The index of the next chunk we expect to receive.
    pub next_chunk: u64,
    /// The handle to the file being written to disk.
    pub file: TokioFile, // Using Tokio's async File
}

/// Holds the entire state of the TUI application.
/// This includes user input, logs, chat history, peer info, UI focus, etc.
#[derive(Debug)]
pub struct App {
    /// A list of log messages displayed in the console pane (keeps the last `MAX_LOG_LINES`).
    pub log: Vec<String>,
    /// The current text typed into the command input box.
    pub input: String,
    /// Where the cursor is located within the command input text.
    pub cursor_position: usize,
    /// The current text typed into the chat input box.
    pub chat_input: String,
    /// Where the cursor is located within the chat input text.
    pub chat_cursor_position: usize,
    /// The current `InputMode` (Normal, Command, or Chat).
    pub input_mode: InputMode,
    /// If `true`, the application should shut down.
    pub exit: bool,
    /// Which pane (`FocusPane`) is currently active.
    pub focused_pane: FocusPane,
    /// How far the console log is scrolled down (0 means scrolled to the top).
    pub console_scroll: usize,
    /// The number of lines visible in the console log area (updates on resize).
    pub console_viewport_height: usize,
    /// The network addresses our application is listening on.
    pub listening_addresses: Vec<Multiaddr>,
    /// The directory where downloaded files will be saved (if set).
    pub download_dir: Option<PathBuf>,
    /// The user's chosen nickname for display in chat and user lists.
    pub nickname: Option<String>,
    /// Our own unique `PeerId`.
    pub local_peer_id: Option<PeerId>,
    /// A map storing information (`PeerInfo`) about discovered peers, keyed by their `PeerId`.
    pub peers: HashMap<PeerId, PeerInfo>,
    /// `true` if we are currently waiting for a ping response.
    pub pinging: bool,
    /// When the last ping command was initiated. Used with `PINGING_DURATION`.
    pub ping_start_time: Option<Instant>,
    /// Whether the user wants to appear as "Online" to other peers.
    pub is_visible: bool,
    /// The current `ChatContext` (Global or Private).
    pub current_chat_context: ChatContext,
    /// Stores the history of messages for the global chat.
    pub global_chat_history: Vec<ChatMessage>,
    /// How far the chat message history is scrolled down.
    pub chat_scroll: usize,
    /// The number of lines visible in the chat message area (updates on resize).
    pub chat_viewport_height: usize,
    /// Stores the history of private messages, keyed by the `PeerId` of the other participant.
    pub private_chat_histories: HashMap<PeerId, Vec<PrivateChatItem>>,
    /// Stores details of the latest pending file offer received from each peer.
    pub pending_offers: HashMap<PeerId, PendingOfferDetails>,
    /// Stores the state of ongoing downloads, keyed by PeerId and then filename.
    pub download_states: HashMap<PeerId, HashMap<String, DownloadState>>,
    /// Stores the local path for files we are currently sending, keyed by (PeerId, filename).
    pub outgoing_transfers: HashMap<(PeerId, String), PathBuf>,
}

// Provides default values for the `App` state when the application starts.
impl Default for App {
    fn default() -> Self {
        // Generate a username with a random 4-digit number
        // e.g. user9350
        let mut rng = rand::rng();
        let random_number: u16 = rng.random_range(0..10000);
        let nickname = format!("user{:04}", random_number);

        App {
            log: Vec::new(), // Start with an empty log
            input: String::new(), // Start with empty command input
            cursor_position: 0, // Cursor at the start
            chat_input: String::new(), // Start with empty chat input
            chat_cursor_position: 0, // Cursor at the start
            input_mode: InputMode::default(), // Start in Normal mode
            exit: false, // Don't exit yet
            focused_pane: FocusPane::default(), // Start with Console focused
            console_scroll: 0, // Start scrolled to the top
            console_viewport_height: 2, // Small default height
            listening_addresses: Vec::new(), // No known addresses initially
            download_dir: None, // No download directory set initially
            nickname: Some(nickname), // Use the generated nickname
            local_peer_id: None, // We don't know our PeerId yet
            peers: HashMap::new(), // No known peers initially
            pinging: false, // Not pinging initially
            ping_start_time: None, // No ping started yet
            is_visible: true, // Appear online by default
            current_chat_context: ChatContext::Global, // Start in global chat
            global_chat_history: Vec::new(), // Empty global chat
            chat_scroll: 0, // Start chat scrolled to the top
            chat_viewport_height: 2, // Small default chat height
            private_chat_histories: HashMap::new(), // No private chats yet
            pending_offers: HashMap::new(), // No pending offers initially
            download_states: HashMap::new(), // No ongoing downloads initially
            outgoing_transfers: HashMap::new(), // No outgoing transfers initially
        }
    }
}

// Limit how many lines we keep in the console log to prevent using too much memory.
const MAX_LOG_LINES: usize = 1000;

// Methods for updating the `App` state.
impl App {
    /// Adds a regular message to the console log.
    /// Ensures the log doesn't exceed `MAX_LOG_LINES` and automatically scrolls down.
    pub fn push<S: Into<String>>(&mut self, line: S) {
        self.log.push(line.into());
        // If the log is too long, remove the oldest message(s).
        if self.log.len() > MAX_LOG_LINES {
            self.log.drain(0..self.log.len() - MAX_LOG_LINES);
            // Adjust scroll if necessary when lines are removed from the top
            // This logic might not be strictly needed if we always scroll down,
            // but it doesn't hurt.
            let max_scroll = self.log.len().saturating_sub(1);
            self.console_scroll = self.console_scroll.min(max_scroll);
        }
        // Auto-scroll to the bottom so the latest message is visible.
        // We calculate the scroll position based on the log length and viewport height.
        let viewport = self.console_viewport_height.max(1); // Ensure viewport height is at least 1
        let new_scroll_pos = self.log.len().saturating_sub(viewport);
        self.console_scroll = new_scroll_pos;
    }

    /// Adds a message prefixed with "[LOG]" to the console log.
    /// Useful for distinguishing internal log messages from user commands/output.
    /// Also ensures the log doesn't exceed `MAX_LOG_LINES` and auto-scrolls.
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

    // --- Command Input Handling ---
    // These methods manage the text and cursor in the command input box.

    /// Moves the command input cursor one character left.
    pub fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.cursor_position.saturating_sub(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_left);
    }

    /// Moves the command input cursor one character right.
    pub fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.cursor_position.saturating_add(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_right);
    }

    /// Inserts a character into the command input at the cursor position.
    pub fn enter_char(&mut self, new_char: char) {
        // Find the correct byte index for inserting (handles multi-byte characters).
        let index = self.byte_index();
        self.input.insert(index, new_char);
        // Move the cursor after the inserted character.
        self.move_cursor_right();
    }

    /// Deletes the character *before* the command input cursor (Backspace key).
    pub fn delete_char(&mut self) {
        // Can't delete if the cursor is already at the beginning.
        let is_not_cursor_leftmost = self.cursor_position != 0;
        if is_not_cursor_leftmost {
            // Rebuild the string without the character before the cursor.
            let current_index = self.cursor_position;
            let from_left_to_current_index = current_index - 1;
            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.input.chars().skip(current_index);
            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    /// Helper function to get the byte index in the string corresponding
    /// to the character-based cursor position. Necessary because Rust strings
    /// are UTF-8, and characters can take up multiple bytes.
    fn byte_index(&self) -> usize {
        self.input
            .char_indices() // Get iterator of (byte_index, char)
            .map(|(i, _)| i) // Keep only the byte indices
            .nth(self.cursor_position) // Find the index corresponding to the character cursor position
            .unwrap_or(self.input.len()) // If cursor is at the end, use the string length
    }

    /// Helper function to ensure the cursor position stays within the valid range
    /// of character indices (0 to number of characters).
    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    /// Moves the command input cursor to the very beginning (position 0).
    pub fn reset_cursor(&mut self) {
        self.cursor_position = 0;
    }

    /// Processes the command currently in the input box when Enter is pressed.
    /// It logs the command, clears the input, returns to Normal mode,
    /// and sends an `AppEvent` to the main loop for handling the command logic.
    pub fn submit_command(&mut self) -> Option<AppEvent> {
        // Add the entered command to the console log for history.
        self.push(format!("> {}", self.input));

        // Make a copy because `process_command` might need mutable access to `self`.
        let input_copy = self.input.clone();
        // Remove the leading '/' if present, otherwise use the whole input.
        let command_input = input_copy.strip_prefix('/').unwrap_or(&input_copy);

        // Delegate the actual command parsing and execution logic.
        let event_to_send = crate::commands::process_command(command_input, self);

        // Clear the input field and reset the cursor.
        self.input.clear();
        self.reset_cursor();
        // Go back to normal mode, ready for the next command or focus change.
        self.input_mode = InputMode::Normal;

        // Return the event (e.g., Dial, Quit) for the main loop to handle.
        event_to_send
    }

    // --- Chat Input Handling ---
    // Similar methods for managing the text and cursor in the chat input box.

    /// Moves the chat input cursor one character left.
    pub fn move_chat_cursor_left(&mut self) {
        let cursor_moved_left = self.chat_cursor_position.saturating_sub(1);
        self.chat_cursor_position = self.clamp_chat_cursor(cursor_moved_left);
    }

    /// Moves the chat input cursor one character right.
    pub fn move_chat_cursor_right(&mut self) {
        let cursor_moved_right = self.chat_cursor_position.saturating_add(1);
        self.chat_cursor_position = self.clamp_chat_cursor(cursor_moved_right);
    }

    /// Inserts a character into the chat input at the cursor position.
    pub fn enter_chat_char(&mut self, new_char: char) {
        let index = self.chat_byte_index();
        self.chat_input.insert(index, new_char);
        self.move_chat_cursor_right();
    }

    /// Deletes the character *before* the chat input cursor (Backspace key).
    pub fn delete_chat_char(&mut self) {
        let is_not_cursor_leftmost = self.chat_cursor_position != 0;
        if is_not_cursor_leftmost {
            let current_index = self.chat_cursor_position;
            let from_left_to_current_index = current_index - 1;
            let before_char_to_delete = self.chat_input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.chat_input.chars().skip(current_index);
            self.chat_input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_chat_cursor_left();
        }
    }

    /// Helper to get the byte index for the chat cursor position.
    fn chat_byte_index(&self) -> usize {
        self.chat_input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.chat_cursor_position)
            .unwrap_or(self.chat_input.len())
    }

    /// Helper to clamp the chat cursor position.
    fn clamp_chat_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.chat_input.chars().count())
    }

    /// Moves the chat input cursor to the beginning.
    pub fn reset_chat_cursor(&mut self) {
        self.chat_cursor_position = 0;
    }

    // --- Rendering Helper Functions ---
    // These functions draw the different parts of the UI.

    /// Draws the console pane, which includes the log messages and the command input box.
    fn render_console_pane(&self, area: Rect, buf: &mut Buffer) {
        // Style to use for the border when this pane is focused.
        let focused_style = Style::default().fg(Color::Yellow);
        let unfocused_style = Style::default(); // Style when not focused.

        // Help text shown at the bottom depends on the current input mode.
        let console_title_bottom = match self.input_mode {
            InputMode::Normal => " Focus: Tab | Scroll: ↑/↓ | Quit: Ctrl+Q ".bold(),
            InputMode::Command => " Submit: Enter | Cancel: Esc ".bold(),
            // While in Chat mode, console hints might still be relevant if user tabs back.
            InputMode::Chat => " Focus: Tab | Scroll: ↑/↓ | Quit: Ctrl+Q ".bold(),
        };
        // Create the main block for the console area.
        let console_block = Block::bordered()
            .title(" Console ".bold()) // Title at the top.
            .title_bottom(Line::from(console_title_bottom)) // Help text at the bottom.
            .border_set(border::THICK) // Use thick borders.
            // Highlight border if this pane is focused.
            .border_style(if self.focused_pane == FocusPane::Console { focused_style } else { unfocused_style });

        // Divide the console area vertically: one part for logs, one for input.
        let console_inner_area = console_block.inner(area); // Get area inside the border.
        let console_chunks = Layout::vertical([
            Constraint::Min(1),      // Log area takes remaining space (at least 1 line).
            Constraint::Length(3), // Input area is fixed at 3 lines high (border + text + border).
        ])
        .split(console_inner_area);

        let log_area = console_chunks[0];
        let input_area = console_chunks[1];

        // Draw the console block's borders and titles onto the buffer.
        console_block.render(area, buf);

        // --- Render Log Messages ---
        // Convert log lines (Vec<String>) into `ratatui` `Text` objects.
        let log_text: Vec<Line> = self.log.iter().map(|l| Line::from(l.clone())).collect();
        // Create a Paragraph widget to display the log text.
        let log_paragraph = Paragraph::new(Text::from(log_text))
            // Apply the current scroll offset.
            .scroll((self.console_scroll as u16, 0));
        // Draw the log paragraph into its designated area.
        log_paragraph.render(log_area, buf);

        // --- Render Command Input Box ---
        // Title changes slightly if a ping is in progress.
        let input_title = if self.pinging {
            " Input (Pinging...) "
        } else {
            " Command Input (/) "
        };
        // Create the paragraph for the input text.
        let input_paragraph = Paragraph::new(self.input.as_str())
            // Style the input text itself (e.g., yellow when in Command mode).
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Command => Style::default().fg(Color::Yellow),
                InputMode::Chat => Style::default(),
            })
            // Put the input text inside its own bordered box with a title.
            .block(Block::bordered().title(input_title.bold()));
        // Draw the input paragraph into its area.
        input_paragraph.render(input_area, buf);
    }

    /// Draws the pane displaying the list of discovered users and their status.
    fn render_users_pane(&self, area: Rect, buf: &mut Buffer) {
        let focused_style = Style::default().fg(Color::Yellow);
        let unfocused_style = Style::default();
        let is_focused = self.focused_pane == FocusPane::UsersList;

        // Create the main block for the users list area.
        let users_block = Block::bordered()
            .title(" Users ".bold())
            .border_set(border::THICK)
            .border_style(if is_focused { focused_style } else { unfocused_style });

        // Get the area inside the border to draw the list.
        let inner_area = users_block.inner(area);
        // Draw the block's borders and title first.
        users_block.render(area, buf);

        // --- Prepare the list items ---
        let mut items: Vec<ListItem> = Vec::new();

        // Add "You" (the local user) to the top of the list.
        if let Some(local_id) = self.local_peer_id {
            let id_str = local_id.to_base58();
            let len = id_str.len();
            // Show only the last 6 chars of the PeerId for brevity.
            let start_index = len.saturating_sub(6);
            let id_suffix = format!("(...{})", &id_str[start_index..]);
            let display_name = format!("You {}", id_suffix);
            // Set prefix and style based on visibility status
            let (prefix, status_style) = if self.is_visible {
                ("[✓] ", Style::default().fg(Color::Green)) // Green check for visible/online
            } else {
                ("[✗] ", Style::default().fg(Color::Gray))   // Gray X for invisible/offline
            };
            let line = Line::from(vec![
                Span::styled(prefix, status_style),
                Span::raw(display_name),
            ]);
            items.push(ListItem::new(line));
        }

        // Separate known peers into online and offline groups.
        let mut online_peers: Vec<_> = Vec::new();
        let mut offline_peers: Vec<_> = Vec::new();

        for (peer_id, peer_info) in self.peers.iter() {
            // Don't list ourselves in the peer list.
            if Some(*peer_id) == self.local_peer_id {
                continue;
            }
            match peer_info.status {
                OnlineStatus::Online => online_peers.push((peer_id, peer_info)),
                OnlineStatus::Offline => offline_peers.push((peer_id, peer_info)),
            }
        }

        // Sort peers alphabetically within each group based on their PeerId string.
        online_peers.sort_by_key(|(id, _)| id.to_base58());
        offline_peers.sort_by_key(|(id, _)| id.to_base58());

        // Helper function to create a `ListItem` for a peer.
        let create_list_item = |peer_id: &PeerId, peer_info: &PeerInfo| {
            let id_str = peer_id.to_base58();
            let len = id_str.len();
            let start_index = len.saturating_sub(6);
            let id_suffix = format!("(...{})", &id_str[start_index..]);

            // Use nickname if available, otherwise "Unknown User".
            let display_name = match &peer_info.nickname {
                Some(nickname) => format!("{} {}", nickname, id_suffix),
                None => format!("Unknown {}", id_suffix), // Shorter "Unknown"
            };

            // Set status indicator based on `peer_info.status`.
            let (prefix, status_style) = match peer_info.status {
                OnlineStatus::Online => ("[✓] ", Style::default().fg(Color::Green)),
                OnlineStatus::Offline => ("[✗] ", Style::default().fg(Color::Gray)),
            };

            let line = Line::from(vec![
                Span::styled(prefix, status_style),
                Span::raw(display_name),
            ]);
            ListItem::new(line)
        };

        // Add online peers to the list items.
        for (peer_id, peer_info) in online_peers {
            items.push(create_list_item(peer_id, peer_info));
        }

        // Add offline peers below the online ones.
        for (peer_id, peer_info) in offline_peers {
            items.push(create_list_item(peer_id, peer_info));
        }

        // Create the `List` widget from the prepared items.
        // Note: Scrolling is not implemented for this list yet.
        let users_list = List::new(items);
        // Draw the list into the inner area of the users block.
        users_list.render(inner_area, buf);
    }

    /// Draws the chat pane, including the message history and the chat input box.
    fn render_chat_pane(&self, area: Rect, buf: &mut Buffer) {
        let focused_style = Style::default().fg(Color::Yellow);
        let unfocused_style = Style::default();
        let is_focused = self.focused_pane == FocusPane::Chat;

        // Title changes depending on whether it's global or private chat.
        let chat_title_text = match &self.current_chat_context {
            ChatContext::Global => " Global Chat ".to_string(),
            // Show nickname in title if available.
            ChatContext::Private { target_nickname: Some(nick), .. } => format!(" Private Chat ({}) ", nick),
            // Fallback if nickname isn't known for the private chat partner.
            ChatContext::Private { .. } => " Private Chat (Unknown User) ".to_string(),
        };
        let chat_title = chat_title_text.bold();

        // Create the main block for the chat area.
        let chat_block = Block::bordered()
            .title(chat_title)
            .border_set(border::THICK)
            .border_style(if is_focused { focused_style } else { unfocused_style });

        // Divide the chat area vertically: messages and input box.
        let chat_inner_area = chat_block.inner(area);
        // Draw the block borders and title first.
        chat_block.render(area, buf);

        let chat_chunks = Layout::vertical([
            Constraint::Min(1),      // Message area takes remaining space.
            Constraint::Length(3), // Chat input area is fixed height.
        ])
        .split(chat_inner_area);

        let messages_area = chat_chunks[0];
        let input_area = chat_chunks[1];

        // --- Render Chat Messages ---
        // Helper function to format a single `ChatMessage` into a display `Line`.
        // Takes ownership of data it needs to avoid lifetime issues with borrows inside the map closure.
        let format_message_line = |msg: &ChatMessage, local_peer_id: Option<PeerId>| -> Line {
            // Display "You" for messages sent by the local user.
            let sender_display: String = if Some(msg.sender_id) == local_peer_id {
                "You".to_string()
            } else {
                // Otherwise, use nickname or a shortened PeerId.
                msg.sender_nickname.clone().unwrap_or_else(|| {
                    let id_str = msg.sender_id.to_base58();
                    let len = id_str.len();
                    // Show "user(...last6)" if nickname is unknown.
                    format!("user(...{})", &id_str[len.saturating_sub(6)..])
                })
            };

            // Clone the message content needed for the `Span`.
            let content_owned: String = msg.content.clone();

            // Construct the line: "Sender: Message Content"
            Line::from(vec![
                Span::styled(format!("{}: ", sender_display), Style::default().bold()), // Sender bold
                Span::raw(content_owned), // Message content normal
            ])
        };

        // Get the relevant message history based on the current chat context.
        let messages: Vec<Line>;
        match &self.current_chat_context {
            ChatContext::Global => {
                // Use global history. Show placeholder if empty.
                if self.global_chat_history.is_empty() {
                    messages = vec![Line::from("No messages yet in global chat.".italic())];
                } else {
                    messages = self.global_chat_history.iter()
                        .map(|msg| format_message_line(msg, self.local_peer_id))
                        .collect();
                }
            }
            ChatContext::Private { target_peer_id, .. } => {
                // Look up private history for the target peer. Show placeholder if none exists.
                if let Some(history) = self.private_chat_histories.get(target_peer_id) {
                    if history.is_empty() {
                        messages = vec![Line::from("No messages yet in this private chat.".italic())];
                    } else {
                        let mut all_lines: Vec<Line> = Vec::with_capacity(history.len()); // Pre-allocate roughly
                        for item in history.iter() {
                            match item {
                                PrivateChatItem::Message(msg) => {
                                    // Use the existing helper for messages
                                    all_lines.push(format_message_line(msg, self.local_peer_id));
                                }
                                PrivateChatItem::Offer(offer_details) => {
                                    // Format the offer details into two lines
                                    let sender_display = self.peers.get(target_peer_id)
                                        .and_then(|p| p.nickname.clone())
                                        .unwrap_or_else(|| {
                                             let id_str = target_peer_id.to_base58();
                                             let len = id_str.len();
                                             format!("user(...{})", &id_str[len.saturating_sub(6)..])
                                        });
                                    // Line 1: Offer details
                                    all_lines.push(Line::from(vec![
                                        Span::styled(">> ", Style::default().fg(Color::Blue)),
                                        Span::styled(format!("{}", sender_display), Style::default().bold()),
                                        Span::raw(format!(
                                            " offered file: '{}' ({}).",
                                            offer_details.filename,
                                            crate::utils::format_bytes(offer_details.size_bytes)
                                        )),
                                    ]));
                                    // Line 2: Prompt
                                    all_lines.push(Line::from(vec![
                                        Span::raw("   "), // Indentation
                                        Span::styled("Use /accept or /decline.", Style::default().fg(Color::Yellow).italic()),
                                    ]));
                                }
                                PrivateChatItem::OfferSent(offer_details) => {
                                    // Format the sent offer details into a single line
                                    all_lines.push(Line::from(vec![
                                        Span::styled(">> ", Style::default().fg(Color::Blue)),
                                        Span::styled("You", Style::default().bold()),
                                        Span::raw(format!(
                                            " offered file: '{}' ({}).",
                                            offer_details.filename,
                                            crate::utils::format_bytes(offer_details.size_bytes)
                                        )),
                                    ]));
                                }
                                PrivateChatItem::OfferDeclined(offer_details) => {
                                    // Format the declined offer details into a single line
                                    all_lines.push(Line::from(vec![
                                        Span::styled("<< ", Style::default().fg(Color::Red)),
                                        Span::styled("You", Style::default().bold()),
                                        Span::raw(format!(
                                            " declined file: '{}' ({}).",
                                            offer_details.filename,
                                            crate::utils::format_bytes(offer_details.size_bytes)
                                        )),
                                    ]));
                                }
                                PrivateChatItem::RemoteOfferDeclined(offer_details) => {
                                    // Format the remotely declined offer details
                                    let peer_display_name = self.peers.get(target_peer_id)
                                        .and_then(|p| p.nickname.clone())
                                        .unwrap_or_else(|| {
                                            let id_str = target_peer_id.to_base58();
                                            let len = id_str.len();
                                            format!("user(...{})", &id_str[len.saturating_sub(6)..])
                                        });
                                    all_lines.push(Line::from(vec![
                                        Span::styled("<< ", Style::default().fg(Color::Red)), // Different indicator
                                        Span::styled(format!("{}", peer_display_name), Style::default().bold()),
                                        Span::raw(format!(
                                            " declined file: '{}' ({}).",
                                            offer_details.filename,
                                            crate::utils::format_bytes(offer_details.size_bytes)
                                        )),
                                    ]));
                                }
                                PrivateChatItem::OfferAccepted(offer_details) => {
                                    // Find the peer's display name
                                    let target_peer_id = match &self.current_chat_context {
                                        ChatContext::Private { target_peer_id, .. } => *target_peer_id,
                                        _ => { /* Should not happen in this arm */ return; }
                                    };
                                    let peer_display_name = self.peers.get(&target_peer_id)
                                        .and_then(|p| p.nickname.clone())
                                        .unwrap_or_else(|| {
                                            let id_str = target_peer_id.to_base58();
                                            let len = id_str.len();
                                            format!("user(...{})", &id_str[len.saturating_sub(6)..])
                                        });
                                    // Format the remotely accepted offer details
                                    all_lines.push(Line::from(vec![
                                        Span::styled(">> ", Style::default().fg(Color::Green)), // Use Green for accepted
                                        Span::styled(format!("{}", peer_display_name), Style::default().bold()),
                                        Span::raw(format!(
                                            " accepted file: '{}' ({}).",
                                            offer_details.filename,
                                            crate::utils::format_bytes(offer_details.size_bytes)
                                        )),
                                    ]));
                                }
                                PrivateChatItem::RemoteOfferAccepted(offer_details) => {
                                    // Find the peer's display name
                                    let target_peer_id = match &self.current_chat_context {
                                        ChatContext::Private { target_peer_id, .. } => *target_peer_id,
                                        _ => { /* Should not happen in this arm */ return; }
                                    };
                                    let peer_display_name = self.peers.get(&target_peer_id)
                                        .and_then(|p| p.nickname.clone())
                                        .unwrap_or_else(|| {
                                            let id_str = target_peer_id.to_base58();
                                            let len = id_str.len();
                                            format!("user(...{})", &id_str[len.saturating_sub(6)..])
                                        });
                                    // Format the remotely accepted offer details
                                    all_lines.push(Line::from(vec![
                                        Span::styled(">> ", Style::default().fg(Color::Green)), // Use Green for accepted
                                        Span::styled(format!("{}", peer_display_name), Style::default().bold()),
                                        Span::raw(format!(
                                            " accepted file: '{}' ({}).",
                                            offer_details.filename,
                                            crate::utils::format_bytes(offer_details.size_bytes)
                                        )),
                                    ]));
                                }
                                PrivateChatItem::TransferProgress { filename, received, total } => {
                                    all_lines.push(Line::from(vec![
                                        Span::styled(">> ", Style::default().fg(Color::Blue)),
                                        Span::styled(format!("{}", filename), Style::default().bold()),
                                        Span::raw(format!(
                                            " in progress: {} / {} bytes",
                                            received,
                                            total
                                        )),
                                    ]));
                                }
                                PrivateChatItem::TransferComplete { filename, final_path, size } => {
                                    all_lines.push(Line::from(vec![
                                        Span::styled(">> ", Style::default().fg(Color::Green)),
                                        Span::styled(format!("{}", filename), Style::default().bold()),
                                        Span::raw(format!(
                                            " completed: {} bytes, saved to {}",
                                            size,
                                            final_path.display()
                                        )),
                                    ]));
                                }
                                PrivateChatItem::TransferFailed { filename, error } => {
                                    all_lines.push(Line::from(vec![
                                        Span::styled("<< ", Style::default().fg(Color::Red)),
                                        Span::styled(format!("{}", filename), Style::default().bold()),
                                        Span::raw(format!(
                                            " failed: {}",
                                            error
                                        )),
                                    ]));
                                }
                            }
                        }
                        messages = all_lines; // Assign the collected lines
                    }
                } else {
                    // No history exists *at all* for this peer yet.
                    messages = vec![Line::from("No messages yet in this private chat.".italic())];
                }
            }
        }

        // Create the paragraph for the chat messages.
        let chat_paragraph = Paragraph::new(Text::from(messages))
            .scroll((self.chat_scroll as u16, 0)); // Apply scroll offset.
        // Draw the messages.
        chat_paragraph.render(messages_area, buf);

        // --- Render Chat Input Box ---
        let chat_input_paragraph = Paragraph::new(self.chat_input.as_str())
            // Highlight text yellow when chat input is active.
            .style(match self.input_mode {
                InputMode::Chat => Style::default().fg(Color::Yellow),
                _ => Style::default(),
            })
            // Put it in its own bordered box.
            .block(Block::bordered().title(" Chat Input ".bold()));
        // Draw the chat input box.
        chat_input_paragraph.render(input_area, buf);
    }
}

// This tells `ratatui` how to draw the entire `App` state.
// It delegates the drawing of each pane to the helper methods above.
impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Calculate the areas for each pane using the layout function.
        let (chat_area, console_area, users_area) = layout_chunks(area);

        // Call the rendering function for each pane.
        self.render_chat_pane(chat_area, buf);
        self.render_console_pane(console_area, buf);
        self.render_users_pane(users_area, buf);

        // Important: Setting the actual cursor position in the terminal
        // needs to happen in the main loop (`main.rs`) because it requires
        // access to the `Frame` object provided by `ratatui`'s drawing cycle.
    }
}

/// Represents different events that can happen in the application,
/// either triggered by user input or by network activity.
/// These events drive the state changes in the main application loop.
#[derive(Debug)]
pub enum AppEvent {
    /// A key was pressed by the user.
    Input(event::KeyEvent),
    /// An event occurred in the underlying libp2p network layer.
    Swarm(SwarmEvent<SwapBytesBehaviourEvent>),
    /// User wants to connect to a specific peer address (from command input).
    Dial(Multiaddr),
    /// A message needs to be displayed in the console log (often from network task).
    LogMessage(String),
    /// A new peer was found on the local network via mDNS.
    PeerDiscovered(PeerId),
    /// A peer previously found via mDNS hasn't been seen for a while and is considered gone.
    PeerExpired(PeerId),
    /// User wants to exit the application (e.g., pressed Ctrl+Q or typed /quit).
    Quit,
    /// User pressed Enter in the chat input box, submitting a message.
    EnterChat(String),
    /// User changed their nickname (needs to be broadcast to others).
    NicknameUpdated(PeerId, String),
    /// User changed their visibility status (online/offline).
    VisibilityChanged(bool),
    /// Received a chat message from the global topic.
    GlobalMessageReceived {
        sender_id: PeerId,
        sender_nickname: Option<String>,
        content: String,
        timestamp_ms: u64,
    },
    /// UI requests the network task to publish a message to the global chat topic.
    PublishGossipsub(Vec<u8>), // Raw bytes because Gossipsub deals with bytes
    /// UI requests the network task to send a private message to a specific peer.
    SendPrivateMessage { target_peer: PeerId, message: String },
    /// UI requests the network task to send a file offer to a specific peer.
    SendFileOffer { target_peer: PeerId, file_path: PathBuf }, // Send PathBuf for now
    /// UI requests the network task to send a decline message for an offer.
    DeclineFileOffer { target_peer: PeerId, filename: String },
    /// UI requests the network task to send an accept message for an offer.
    SendAcceptOffer { target_peer: PeerId, filename: String, size_bytes: u64 },
    /// Received a private chat message directly from a peer.
    PrivateMessageReceived { sender_id: PeerId, content: String },
    /// Received a file offer directly from a peer.
    FileOfferReceived {
        sender_id: PeerId,
        filename: String,
        size_bytes: u64,
    },
    /// Received confirmation that a peer declined a file offer we sent.
    FileOfferDeclined { peer_id: PeerId, filename: String },
    /// Received confirmation that a peer accepted a file offer we sent.
    FileOfferAccepted { peer_id: PeerId, filename: String },
    /// Reports progress of an ongoing file download.
    FileTransferProgress {
        peer_id: PeerId,
        filename: String,
        received: u64, // Bytes received so far
        total: u64,    // Total file size in bytes
    },
    /// Indicates a file transfer has completed successfully.
    FileTransferComplete {
        peer_id: PeerId,
        filename: String,
        path: PathBuf, // Final path where the file was saved
        total_size: u64, // <<< Add total size
    },
    /// Indicates a file transfer has failed.
    FileTransferFailed {
        peer_id: PeerId,
        filename: String,
        error: String, // Reason for failure
    },
    /// UI informs swarm task of the current download directory.
    DownloadDirChanged(Option<PathBuf>),
    /// UI informs swarm task to register an outgoing file transfer.
    RegisterOutgoingTransfer {
        peer_id: PeerId,
        filename: String,
        path: PathBuf
    },
}

// Helper function to divide the main terminal area into the three panes:
// Chat (top-left), Console (bottom-left), Users (right).
pub fn layout_chunks(area: Rect) -> (Rect, Rect, Rect) {
    // Split horizontally: 75% for left side (Chat + Console), 25% for Users list.
    let main_chunks = Layout::horizontal([
        Constraint::Percentage(75),
        Constraint::Percentage(25),
    ])
    .split(area);
    let left_area = main_chunks[0];
    let users_area = main_chunks[1];

    // Split the left side vertically: 67% for Chat, 33% for Console.
    let left_chunks = Layout::vertical([
        Constraint::Percentage(67),
        Constraint::Percentage(33),
    ])
    .split(left_area);
    let chat_area = left_chunks[0];
    let console_area = left_chunks[1];

    // Return the calculated areas for each pane.
    (chat_area, console_area, users_area)
}
