/*
TUI related components for the libp2p ping application.
*/

// TUI imports
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    symbols::border,
    text::{Line, Text},
    widgets::{Block, Paragraph, Widget},
    layout::{Constraint, Layout},
    style::{Color, Style},
};
use crossterm::event;
use std::path::PathBuf;

// libp2p imports
use libp2p::{ping, swarm::SwarmEvent};
use libp2p::Multiaddr;

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

        let command_parts: Vec<&str> = self.input.trim().splitn(2, ' ').collect();
        let command_name = *command_parts.get(0).unwrap_or(&"");
        let args = command_parts.get(1).unwrap_or(&"").trim(); // Trim args

        let mut event_to_send = None;

        match command_name {
            "/ping" => {
                if args.is_empty() {
                    self.push("Usage: /ping <multiaddr>".to_string());
                } else {
                    match args.parse::<Multiaddr>() {
                        Ok(addr) => {
                            event_to_send = Some(AppEvent::Dial(addr));
                        }
                        Err(e) => {
                            self.push(format!("Invalid Multiaddr: {e}"));
                        }
                    }
                }
            }
            "/me" => {
                // Show listening addresses
                self.push("You are listening on addresses:".to_string());
                if self.listening_addresses.is_empty() {
                    self.push("  (Not listening on any addresses right now)".to_string());
                } else {
                    let addrs_to_print: Vec<String> = self.listening_addresses
                        .iter()
                        .map(|addr| format!("  {}", addr))
                        .collect();
                    for addr_str in addrs_to_print {
                        self.push(addr_str);
                    }
                }
                 // Show download directory if set
                match &self.download_dir {
                    Some(dir) => self.push(format!("Download directory: {}", dir.display())),
                    None => self.push("Download directory: (Not set - use /setdir)".to_string()),
                }
                // Show nickname if set
                match &self.nickname {
                    Some(name) => self.push(format!("Nickname: {}", name)),
                    None => self.push("Nickname: (Not set - use /setname)".to_string()),
                }
            }
            "/setdir" => {
                if args.is_empty() {
                    self.push("Usage: /setdir <absolute_path>".to_string());
                } else {
                    // Call the verification function from utils
                    // Note: This blocks briefly. For heavy I/O, consider spawning a task.
                    match crate::utils::verify_download_directory(args) {
                        Ok(verified_path) => {
                            self.push(format!("Download directory set to: {}", verified_path.display()));
                            self.download_dir = Some(verified_path);
                        }
                        Err(err_msg) => {
                            self.push(format!("Error setting directory: {}", err_msg));
                        }
                    }
                }
            }
            "/setname" => {
                if args.is_empty() {
                    self.push("Usage: /setname <nickname>".to_string());
                } else {
                    // Call the verification function from utils
                    match crate::utils::verify_nickname(args) {
                        Ok(verified_name) => {
                            self.push(format!("Nickname set to: {}", verified_name));
                            self.nickname = Some(verified_name);
                            // TODO: Broadcast nickname change to network
                        }
                        Err(err_msg) => {
                            self.push(format!("Error setting nickname: {}", err_msg));
                        }
                    }
                }
            }
            "/quit" | "/q" => {
                event_to_send = Some(AppEvent::Quit);
            }
            "/help" | "/h" => {
                self.push("SwapBytes Commands:".to_string());
                self.push("  /me               - Show my info (addrs, dir, nickname).".to_string());
                self.push("  /setdir <path>    - Set the absolute path for downloads.".to_string());
                self.push("  /setname <name>   - Set your nickname (3-16 chars, a-z, A-Z, 0-9, -, _).".to_string());
                self.push("  /ping <multiaddr> - Ping a peer.".to_string());
                self.push("  /quit             - Exit SwapBytes.".to_string());
                // Add other commands here as needed
                self.push("  /help             - Show this help message.".to_string());
            }
            // Unknown command
            _ => {
                 if !command_name.is_empty() { // Only show unknown if not empty
                    self.push(format!("Unknown command: {}", command_name));
                    self.push("Type /help for a list of commands.".to_string());
                }
            }
        }

        self.input.clear();
        self.reset_cursor();
        self.input_mode = InputMode::Normal; // Return to normal mode after submit

        event_to_send // Return the event for the main loop
    }
}

/// Implements the rendering logic for the `App` state using Ratatui.
impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Style for focused vs unfocused panes
        let focused_style = Style::default().fg(Color::Yellow);
        let unfocused_style = Style::default();

        // --- Overall Layout (Left: Chat/Console, Right: Users) ---
        let main_chunks = Layout::horizontal([
            Constraint::Percentage(75), // Left side (Chat + Console)
            Constraint::Percentage(25), // Right side (Users List)
        ])
        .split(area);

        let left_area = main_chunks[0];
        let right_area = main_chunks[1];

        // --- Left Side Layout (Top: Chat, Bottom: Console) ---
        let left_chunks = Layout::vertical([
            Constraint::Percentage(67), // Top: Chat (approx 2/3)
            Constraint::Percentage(33), // Bottom: Console (approx 1/3)
        ])
        .split(left_area);

        let chat_area = left_chunks[0];
        let console_area = left_chunks[1];

        // --- Placeholder: Users List ---
        let users_block = Block::bordered()
            .title(" Users ".bold())
            .border_set(border::THICK)
            .border_style(if self.focused_pane == FocusPane::UsersList { focused_style } else { unfocused_style });
        users_block.render(right_area, buf);
        // TODO: Render actual user list inside users_block.inner(right_area)


        // --- Placeholder: Chat ---
        let chat_title = " Global Chat ".bold();
        let chat_block = Block::bordered()
            .title(chat_title)
            .border_set(border::THICK)
            .border_style(if self.focused_pane == FocusPane::Chat { focused_style } else { unfocused_style });
        chat_block.render(chat_area, buf);
        // TODO: Render chat messages inside chat_block.inner(chat_area)


        // --- Console Panel (Adapted from previous main block) ---
        let console_title_bottom = match self.input_mode {
            InputMode::Normal => " Focus: Tab | Scroll: ↑/↓ | Quit: Ctrl+Q ".bold(), // Updated hint
            InputMode::Editing => " Submit: Enter | Cancel: Esc ".bold(),
        };
        let console_block = Block::bordered()
            .title(" Console ".bold())
            .title_bottom(Line::from(console_title_bottom))
            .border_set(border::THICK)
            .border_style(if self.focused_pane == FocusPane::Console { focused_style } else { unfocused_style });

        // Layout within the console block (Log + Input)
        let console_inner_area = console_block.inner(console_area);
        let console_chunks = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(console_inner_area);

        let log_area = console_chunks[0];
        let input_area = console_chunks[1];

        // Render the console block border first
        console_block.render(console_area, buf);

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
        let input_paragraph = Paragraph::new(self.input.as_str())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(Color::Yellow),
            })
            .block(Block::bordered().title(" Input (/)"));
        input_paragraph.render(input_area, buf);


        // --- Cursor ---
        // Set cursor position only when in editing mode and within the input area
        match self.input_mode {
            InputMode::Normal => {} // No cursor in normal mode
            #[allow(clippy::cast_possible_truncation)]
            InputMode::Editing => {
                // Make sure cursor isn't rendered outside the visible input box
                // The actual cursor setting happens in main.rs using f.set_cursor()
            }
        }
    }
}

/// Events that drive the application's state changes.
#[derive(Debug)]
pub enum AppEvent {
    /// User keyboard input.
    Input(event::KeyEvent),
    /// Event originating from the libp2p Swarm.
    Swarm(SwarmEvent<ping::Event>),
    /// User command to dial a peer (sent from UI to Swarm task).
    Dial(Multiaddr),
    /// Message to be logged in the UI (sent from Swarm task to UI).
    LogMessage(String),
    /// User command to quit the application.
    Quit,
}
