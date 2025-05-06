/*
Handles keyboard input events for the TUI.
*/

// --- Standard Library Imports ---
use std::time::{SystemTime, UNIX_EPOCH};

// --- External Crates Imports ---
use crossterm::event::{self, KeyCode, KeyEvent, KeyEventKind};
use tokio::sync::mpsc;

// --- Local Module Imports ---
use crate::{
    protocol,
    tui::{self, App, AppEvent, FocusPane, InputMode},
};

// --- Function Definitions ---

/// Processes a single key event received from the terminal.
///
/// Modifies the application state (`app`) based on the key pressed and the current
/// input mode. Sends commands to the network task via `cmd_tx` when necessary
/// (e.g., sending messages, dialing peers).
///
/// Returns `true` if the TUI needs to be redrawn after processing the event,
/// `false` otherwise.
pub fn handle_key_event(
    app: &mut App,
    cmd_tx: &mpsc::UnboundedSender<AppEvent>,
    key: KeyEvent,
) -> bool {
    let mut redraw = false;

    // --- Global Keybindings (Regardless of Mode) ---

    // Ctrl+q: Quit the application
    if key.kind == KeyEventKind::Press
        && key.code == KeyCode::Char('q')
        && key.modifiers.contains(event::KeyModifiers::CONTROL)
    {
        app.exit = true;
        return false; // Signal immediate exit, no redraw needed
    }

    // --- Mode-Specific Keybindings ---
    match app.input_mode {
        // --- Normal Mode ---
        // Used for navigation between panes and entering other modes.
        InputMode::Normal => {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    // Tab: Cycle focus between Console, UsersList, and Chat panes.
                    KeyCode::Tab => {
                        app.focused_pane = match app.focused_pane {
                            FocusPane::Console => FocusPane::UsersList,
                            FocusPane::UsersList => FocusPane::Chat,
                            FocusPane::Chat => FocusPane::Console,
                        };
                        redraw = true;
                    }
                    // '/': Enter Command mode to type commands in the console.
                    KeyCode::Char('/') => {
                        app.focused_pane = FocusPane::Console; // Ensure console has focus
                        app.input_mode = InputMode::Command;
                        app.input.clear();
                        app.input.push('/'); // Pre-fill with '/'
                        app.cursor_position = 1;
                        redraw = true;
                    }
                    // Up/Down Arrow (Console Focus): Scroll console log view.
                    KeyCode::Up if app.focused_pane == FocusPane::Console => {
                        app.console_scroll = app.console_scroll.saturating_sub(1);
                        redraw = true;
                    }
                    KeyCode::Down if app.focused_pane == FocusPane::Console => {
                        let max_scroll = app
                            .log
                            .len()
                            .saturating_sub(app.console_viewport_height);
                        app.console_scroll = app.console_scroll.saturating_add(1).min(max_scroll);
                        redraw = true;
                    }
                    // Up/Down Arrow (Chat Focus): Scroll chat history view.
                    KeyCode::Up if app.focused_pane == FocusPane::Chat => {
                        app.chat_scroll = app.chat_scroll.saturating_sub(1);
                        redraw = true;
                    }
                    KeyCode::Down if app.focused_pane == FocusPane::Chat => {
                        // Calculate max scroll based on the active chat context (Global or Private)
                        let history_len = match &app.current_chat_context {
                            tui::ChatContext::Global => app.global_chat_history.len(),
                            tui::ChatContext::Private { target_peer_id, .. } => app
                                .private_chat_histories
                                .get(target_peer_id)
                                .map_or(0, |h| h.len()),
                        };
                        let max_scroll = history_len.saturating_sub(app.chat_viewport_height);
                        app.chat_scroll = app.chat_scroll.saturating_add(1).min(max_scroll);
                        redraw = true;
                    }
                    // Any Character (Chat Focus): Enter Chat mode to type a message.
                    KeyCode::Char(c) if app.focused_pane == FocusPane::Chat => {
                        app.input_mode = InputMode::Chat;
                        app.chat_input.clear();
                        app.reset_chat_cursor();
                        app.enter_chat_char(c); // Insert the typed character
                        redraw = true;
                    }
                    _ => {} // Ignore other keys in Normal mode
                }
            }
        }
        // --- Command Mode ---
        // Used for typing commands starting with '/' in the console input bar.
        InputMode::Command => {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    // Enter: Submit the entered command for processing.
                    KeyCode::Enter => {
                        // `submit_command` parses the input and returns an `AppEvent` if valid.
                        if let Some(event) = app.submit_command() {
                            match event {
                                // Handle Quit command locally by setting the exit flag.
                                AppEvent::Quit => {
                                    app.exit = true;
                                }
                                // Send other valid commands (e.g., Dial, Nickname) to the swarm task.
                                _ => {
                                    let _ = cmd_tx.send(event); // Ignore potential send error for now
                                }
                            }
                        }
                        // Don't redraw if quitting, otherwise redraw to clear input/show output.
                        if !app.exit {
                            redraw = true;
                        }
                    }
                    // Character Input: Append character to the command input buffer.
                    KeyCode::Char(to_insert) => {
                        app.enter_char(to_insert);
                        redraw = true;
                    }
                    // Backspace: Delete character before the cursor.
                    KeyCode::Backspace => {
                        app.delete_char();
                        redraw = true;
                    }
                    // Left/Right Arrow: Move cursor within the input buffer.
                    KeyCode::Left => {
                        app.move_cursor_left();
                        redraw = true;
                    }
                    KeyCode::Right => {
                        app.move_cursor_right();
                        redraw = true;
                    }
                    // Up/Down Arrow: Scroll the main console log view while typing a command.
                    KeyCode::Up => {
                        app.console_scroll = app.console_scroll.saturating_sub(1);
                        redraw = true;
                    }
                    KeyCode::Down => {
                        let max_scroll = app
                            .log
                            .len()
                            .saturating_sub(app.console_viewport_height);
                        app.console_scroll = app.console_scroll.saturating_add(1).min(max_scroll);
                        redraw = true;
                    }
                    // Esc: Cancel command input and return to Normal mode.
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Normal;
                        app.input.clear();
                        app.reset_cursor();
                        redraw = true;
                    }
                    // Tab: Cancel command input, return to Normal mode, keeping Console focus.
                    KeyCode::Tab => {
                        app.input_mode = InputMode::Normal;
                        app.input.clear();
                        app.reset_cursor();
                        redraw = true; // Redraw needed to clear input bar
                    }
                    _ => {} // Ignore other keys
                }
            }
        }
        // --- Chat Mode ---
        // Used for typing messages in the chat input bar.
        InputMode::Chat => {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    // Enter: Send the typed message (if not empty).
                    KeyCode::Enter => {
                        if !app.chat_input.is_empty() {
                            let local_peer_id = app.local_peer_id.expect("Local PeerID must be set before chatting");
                            let timestamp_ms = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .expect("System time is before UNIX EPOCH")
                                .as_millis()
                                as u64;
                            let content = app.chat_input.clone(); // Clone for potential multiple uses

                            match app.current_chat_context {
                                // --- Sending Global Chat Message ---
                                tui::ChatContext::Global => {
                                    let nickname = app.nickname.clone();
                                    // Construct the network message format.
                                    let message = protocol::Message::GlobalChatMessage {
                                        content: content.clone(),
                                        timestamp_ms,
                                        nickname: nickname.clone(),
                                    };

                                    // Attempt to serialize the message for network transmission.
                                    match serde_json::to_vec(&message) {
                                        Ok(data) => {
                                            // Send an event to the swarm task to publish the message via Gossipsub.
                                            if let Err(e) = cmd_tx.send(AppEvent::PublishGossipsub(data)) {
                                                app.push(format!(
                                                    "Error sending global message event: {}",
                                                    e
                                                ));
                                            }

                                            // Add the sent message to the local global chat history.
                                            let local_chat_msg = tui::ChatMessage {
                                                sender_id: local_peer_id,
                                                sender_nickname: nickname, // Use original nickname
                                                content, // Use original content
                                                timestamp_ms,
                                            };
                                            app.global_chat_history.push(local_chat_msg);

                                            // Auto-scroll the chat view to the bottom if it was already there.
                                            let current_max_scroll = app
                                                .global_chat_history
                                                .len()
                                                .saturating_sub(app.chat_viewport_height.max(1)) // Prevent underflow if height=0
                                                .saturating_sub(1); // Max scroll *before* adding the new message
                                            if app.chat_scroll >= current_max_scroll {
                                                let new_max_scroll = app
                                                    .global_chat_history
                                                    .len()
                                                    .saturating_sub(app.chat_viewport_height.max(1));
                                                app.chat_scroll = new_max_scroll;
                                            }
                                            // Otherwise, keep the user's scrolled position.
                                        }
                                        Err(e) => {
                                            // Log serialization errors locally.
                                            app.push(format!(
                                                "Error serializing global chat message: {}",
                                                e
                                            ));
                                            // Decide whether to clear input on error (currently does clear below).
                                        }
                                    }
                                }
                                // --- Sending Private Chat Message ---
                                tui::ChatContext::Private { target_peer_id, .. } => {
                                    // Send an event to the swarm task to send the message directly to the target peer.
                                    if let Err(e) = cmd_tx.send(AppEvent::SendPrivateMessage {
                                        target_peer: target_peer_id,
                                        message: content.clone(), // Clone message content for event
                                    }) {
                                        app.push(format!(
                                            "Error sending private message event: {}",
                                            e
                                        ));
                                    } else {
                                        // If the event was sent successfully, add the message to local private history.
                                        let chat_msg = tui::ChatMessage {
                                            sender_id: local_peer_id,
                                            sender_nickname: app.nickname.clone(),
                                            content, // Use original content
                                            timestamp_ms,
                                        };

                                        let history = app
                                            .private_chat_histories
                                            .entry(target_peer_id)
                                            .or_default();
                                        let current_len = history.len(); // History length before adding
                                        history.push(tui::PrivateChatItem::Message(chat_msg));

                                        // Auto-scroll this private chat view if it was at the bottom.
                                        let current_max_scroll = current_len
                                            .saturating_sub(app.chat_viewport_height.max(1));
                                        if app.chat_scroll >= current_max_scroll {
                                            let new_max_scroll = history
                                                .len()
                                                .saturating_sub(app.chat_viewport_height.max(1));
                                            app.chat_scroll = new_max_scroll;
                                        }
                                    }
                                }
                            }

                            // Clear input field, reset cursor, and return to Normal mode after sending (or attempting to send).
                            app.chat_input.clear();
                            app.reset_chat_cursor();
                            app.input_mode = InputMode::Normal;
                            redraw = true;
                        } else {
                            // If Enter is pressed with an empty input, just exit Chat mode.
                            app.input_mode = InputMode::Normal;
                            redraw = true; // Redraw needed to clear input bar focus state
                        }
                    }
                    // Character Input: Append character to the chat input buffer.
                    KeyCode::Char(to_insert) => {
                        app.enter_chat_char(to_insert);
                        redraw = true;
                    }
                    // Backspace: Delete character before the cursor in chat input.
                    KeyCode::Backspace => {
                        app.delete_chat_char();
                        redraw = true;
                    }
                    // Left/Right Arrow: Move cursor within the chat input buffer.
                    KeyCode::Left => {
                        app.move_chat_cursor_left();
                        redraw = true;
                    }
                    KeyCode::Right => {
                        app.move_chat_cursor_right();
                        redraw = true;
                    }
                    // Up/Down Arrow: Scroll the main chat history view while typing a message.
                    KeyCode::Up => {
                        app.chat_scroll = app.chat_scroll.saturating_sub(1);
                        redraw = true;
                    }
                    KeyCode::Down => {
                         // Calculate max scroll based on the active chat context
                        let history_len = match &app.current_chat_context {
                            tui::ChatContext::Global => app.global_chat_history.len(),
                            tui::ChatContext::Private { target_peer_id, .. } => app
                                .private_chat_histories
                                .get(target_peer_id)
                                .map_or(0, |h| h.len()),
                        };
                        let max_scroll = history_len.saturating_sub(app.chat_viewport_height);
                        app.chat_scroll = app.chat_scroll.saturating_add(1).min(max_scroll);
                        redraw = true;
                    }
                    // Esc: Cancel chat input and return to Normal mode.
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Normal;
                        app.chat_input.clear();
                        app.reset_chat_cursor();
                        redraw = true;
                    }
                    // Tab: Cancel chat input, return to Normal mode, keeping Chat focus.
                    KeyCode::Tab => {
                        app.input_mode = InputMode::Normal;
                        app.chat_input.clear();
                        app.reset_chat_cursor();
                        redraw = true; // Redraw needed to clear input bar
                    }
                    _ => {} // Ignore other keys
                }
            }
        }
    }

    // Return whether the UI needs to be redrawn based on actions taken.
    redraw
} 