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

// libp2p imports
use libp2p::{ping, swarm::SwarmEvent};
use libp2p::Multiaddr;

/// Input modes for the TUI.
#[derive(Debug)]
pub enum InputMode {
    Normal,
    Editing,
}

impl Default for InputMode {
    fn default() -> Self { InputMode::Normal }
}

/// Application state for the TUI.
#[derive(Default, Debug)]
pub struct App {
    /// Limited history of log messages.
    pub log: Vec<String>,
    /// Current value of the input box.
    pub input: String,
    /// Position of cursor in the input box.
    pub cursor_position: usize,
    /// Current input mode.
    pub input_mode: InputMode,
    /// Flag indicating if the application should exit.
    pub exit: bool,
}

impl App {
    /// Adds a new message line to the log, maintaining a maximum history size.
    pub fn push<S: Into<String>>(&mut self, line: S) {
        const MAX_LOG_LINES: usize = 10;
        self.log.push(line.into());
        if self.log.len() > MAX_LOG_LINES {
            self.log.drain(0..self.log.len() - MAX_LOG_LINES);
        }
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
    fn reset_cursor(&mut self) {
        self.cursor_position = 0;
    }

    /// Submits the current input as a command.
    /// Returns an optional AppEvent if a command needs to be processed by the main loop.
    pub fn submit_command(&mut self) -> Option<AppEvent> {
        self.push(format!("> {}", self.input)); // Log the entered command

        let command_parts: Vec<&str> = self.input.trim().splitn(2, ' ').collect();
        let command_name = command_parts.get(0).unwrap_or(&"");
        let args = command_parts.get(1).unwrap_or(&"");

        let mut event_to_send = None;

        if *command_name == "/ping" {
            if args.is_empty() {
                self.push("Usage: /ping <multiaddr>".to_string());
            } else {
                match args.parse::<Multiaddr>() {
                    Ok(addr) => {
                        // Don't push "Dialing..." here, let the main loop do it
                        // after successfully calling swarm.dial()
                        event_to_send = Some(AppEvent::Dial(addr));
                    }
                    Err(e) => {
                        self.push(format!("Invalid Multiaddr: {e}"));
                    }
                }
            }
        } else if !self.input.trim().is_empty() { // Only show unknown if not empty
            self.push(format!("Unknown command: {}", command_name));
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
        // --- Main Block ---
        let title_bottom = match self.input_mode {
            InputMode::Normal => " (/: Input | Ctrl+q: Quit) ".bold(),
            InputMode::Editing => " (Esc: Exit Input | Enter: Submit) ".bold(),
        };
        let block = Block::bordered()
            .title(" SwapBytes Console ".bold())
            .title_bottom(Line::from(title_bottom))
            .border_set(border::THICK);

        // --- Layout within the main block ---
        // Create a layout for the log panel and the input box
        let inner_area = block.inner(area);
        let chunks = Layout::vertical([
            Constraint::Min(1), // Log panel takes remaining space
            Constraint::Length(3), // Input box is 3 lines high
        ])
        .split(inner_area);

        // Render the main block first to draw borders
        block.render(area, buf);

        // --- Log Panel ---
        let log_text = Text::from(
            self.log
                .iter()
                .map(|l| Line::from(l.clone()))
                .collect::<Vec<_>>(),
        );
        // We don't need a block around the log paragraph itself as it's inside the main block
        Paragraph::new(log_text).render(chunks[0], buf);


        // --- Input Box ---
        let input_paragraph = Paragraph::new(self.input.as_str())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(Color::Yellow),
            })
            .block(Block::bordered().title(" Input "));
        input_paragraph.render(chunks[1], buf);


        // --- Cursor ---
        // Set cursor position only when in editing mode
        match self.input_mode {
            InputMode::Normal => {} // No cursor in normal mode
            #[allow(clippy::cast_possible_truncation)]
            InputMode::Editing => {
                // Cursor position is set in the main loop (main.rs)
                // where the Frame object is available.
            }
        }
    }
}

/// Events that drive the application's state changes.
pub enum AppEvent {
    /// User keyboard input.
    Input(event::KeyEvent),
    /// Event originating from the libp2p Swarm.
    Swarm(SwarmEvent<ping::Event>),
    /// User command to dial a peer (sent from UI to Swarm task).
    Dial(Multiaddr),
    /// Message to be logged in the UI (sent from Swarm task to UI).
    LogMessage(String),
}
