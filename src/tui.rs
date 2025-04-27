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
};
use crossterm::event;

// libp2p imports
use libp2p::{ping, swarm::SwarmEvent};



/// Application state for the TUI.
#[derive(Default, Debug)]
pub struct App {
    /// Limited history of log messages.
    pub log: Vec<String>,
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
}

/// Implements the rendering logic for the `App` state using Ratatui.
impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered()
            .title(Line::from(" libp2p-ping demo ".bold()))
            .title_bottom(Line::from(" q to quit ".bold()))
            .border_set(border::THICK);

        let body = Text::from(
            self.log
                .iter()
                .map(|l| Line::from(l.clone()))
                .collect::<Vec<_>>(),
        );

        Paragraph::new(body).block(block).render(area, buf);
    }
}

/// Events that drive the application's state changes.
pub enum AppEvent {
    /// User keyboard input.
    Input(event::KeyEvent),
    /// Event originating from the libp2p Swarm.
    Swarm(SwarmEvent<ping::Event>),
}
