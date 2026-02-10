//! Multi-selection component with toggle support.

use std::collections::HashSet;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use super::{PromptResult, ValidationResult, wrapped_line_count};
use crate::tui::{TerminalApp, theme::Theme};

/// An item in a multi-selection list.
#[derive(Debug, Clone)]
pub struct MultiSelectItem<T> {
    /// The value returned when this item is selected
    pub value: T,
    /// The label displayed to the user
    pub label: String,
    /// Whether this item can be toggled
    pub enabled: bool,
    /// Whether this item is selected by default
    pub default_selected: bool,
}

impl<T> MultiSelectItem<T> {
    /// Creates a new enabled multi-select item.
    #[must_use]
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            enabled: true,
            default_selected: false,
        }
    }

    /// Creates a new disabled multi-select item.
    #[must_use]
    #[allow(dead_code)]
    pub fn disabled(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            enabled: false,
            default_selected: false,
        }
    }

    /// Sets whether this item is selected by default.
    #[must_use]
    #[allow(dead_code)]
    pub fn selected(mut self, selected: bool) -> Self {
        self.default_selected = selected;
        self
    }
}

/// A multi-selection prompt with toggle support.
pub struct MultiSelect<'a, T, V>
where
    V: Fn(&[&T]) -> ValidationResult,
{
    title: &'a str,
    items: Vec<MultiSelectItem<T>>,
    help_message: Option<&'a str>,
    validator: Option<V>,
    theme: Theme,
}

impl<'a, T> MultiSelect<'a, T, fn(&[&T]) -> ValidationResult> {
    /// Creates a new multi-select prompt with the given title and items.
    #[must_use]
    pub fn new(title: &'a str, items: Vec<MultiSelectItem<T>>) -> Self {
        Self {
            title,
            items,
            help_message: None,
            validator: None,
            theme: Theme::default(),
        }
    }
}

impl<'a, T, V> MultiSelect<'a, T, V>
where
    V: Fn(&[&T]) -> ValidationResult,
{
    /// Sets the help message displayed below the list.
    #[must_use]
    pub fn with_help_message(mut self, message: &'a str) -> Self {
        self.help_message = Some(message);
        self
    }

    /// Sets the validator function for the selection.
    #[must_use]
    pub fn with_validator<NewV>(self, validator: NewV) -> MultiSelect<'a, T, NewV>
    where
        NewV: Fn(&[&T]) -> ValidationResult,
    {
        MultiSelect {
            title: self.title,
            items: self.items,
            help_message: self.help_message,
            validator: Some(validator),
            theme: self.theme,
        }
    }

    /// Sets a custom theme for the multi-select.
    #[must_use]
    #[allow(dead_code)]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Runs the multi-select prompt and returns the selected items' values.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(values))` - User submitted selections
    /// - `Ok(None)` - User cancelled with Escape
    /// - `Err(_)` - Terminal error occurred
    pub fn prompt(self) -> PromptResult<Vec<T>> {
        if self.items.is_empty() {
            return Ok(Some(Vec::new()));
        }

        let mut app = TerminalApp::new()?;
        let mut state = ListState::default();
        state.select(Some(0));

        let mut selected_indices: HashSet<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.default_selected && item.enabled)
            .map(|(idx, _)| idx)
            .collect();

        let mut error_message: Option<String> = None;

        loop {
            app.terminal().draw(|frame| {
                self.render(
                    frame,
                    &mut state,
                    &selected_indices,
                    error_message.as_deref(),
                );
            })?;

            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.move_cursor_up(&mut state);
                        error_message = None;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.move_cursor_down(&mut state);
                        error_message = None;
                    }
                    KeyCode::Char(' ') => {
                        if let Some(idx) = state.selected()
                            && let Some(item) = self.items.get(idx)
                            && item.enabled
                        {
                            if selected_indices.contains(&idx) {
                                selected_indices.remove(&idx);
                            } else {
                                selected_indices.insert(idx);
                            }
                            error_message = None;
                        }
                    }
                    KeyCode::Enter => {
                        let selected_values: Vec<&T> = selected_indices
                            .iter()
                            .filter_map(|&idx| self.items.get(idx).map(|item| &item.value))
                            .collect();

                        if let Some(ref validator) = self.validator {
                            match validator(&selected_values) {
                                ValidationResult::Valid => {
                                    // Extract values from items
                                    let items = self.items;
                                    let result: Vec<T> = items
                                        .into_iter()
                                        .enumerate()
                                        .filter(|(idx, _)| selected_indices.contains(idx))
                                        .map(|(_, item)| item.value)
                                        .collect();
                                    return Ok(Some(result));
                                }
                                ValidationResult::Invalid(msg) => {
                                    error_message = Some(msg);
                                }
                            }
                        } else {
                            let items = self.items;
                            let result: Vec<T> = items
                                .into_iter()
                                .enumerate()
                                .filter(|(idx, _)| selected_indices.contains(idx))
                                .map(|(_, item)| item.value)
                                .collect();
                            return Ok(Some(result));
                        }
                    }
                    KeyCode::Esc => return Ok(None),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(None);
                    }
                    KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Select all enabled items
                        for (idx, item) in self.items.iter().enumerate() {
                            if item.enabled {
                                selected_indices.insert(idx);
                            }
                        }
                        error_message = None;
                    }
                    _ => {}
                }
            }
        }
    }

    fn move_cursor_up(&self, state: &mut ListState) {
        let current = state.selected().unwrap_or(0);
        let new_idx = if current == 0 {
            self.items.len().saturating_sub(1)
        } else {
            current.saturating_sub(1)
        };
        state.select(Some(new_idx));
    }

    #[allow(clippy::arithmetic_side_effects)]
    fn move_cursor_down(&self, state: &mut ListState) {
        let current = state.selected().unwrap_or(0);
        let new_idx = (current + 1) % self.items.len();
        state.select(Some(new_idx));
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation
    )]
    fn render(
        &self,
        frame: &mut Frame,
        state: &mut ListState,
        selected_indices: &HashSet<usize>,
        error_message: Option<&str>,
    ) {
        let help_text = self
            .help_message
            .unwrap_or("↑↓ navigate, Space toggle, Enter submit, Esc cancel");

        // Estimate how many rows help/error text need when wrapped to the dialog's inner width.
        // Dialog is 60% of terminal width, minus 2 for borders.
        let dialog_inner_width = frame.area().width * 60 / 100;
        let avail_width = dialog_inner_width.saturating_sub(2);
        let help_lines = wrapped_line_count(help_text, avail_width);
        let error_lines = error_message.map_or(1, |e| wrapped_line_count(e, avail_width));

        // Cap list height to max 15 items visible, plus border(2) + help + error + padding(1)
        let max_visible_items: u16 = 15;
        let list_height = (self.items.len() as u16).min(max_visible_items)
            + 3
            + help_lines
            + error_lines; // items + border(2) + padding(1) + help + error
        let height = list_height.min(frame.area().height.saturating_sub(2));
        let area = centered_rect(60, height, frame.area());

        frame.render_widget(Clear, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.theme.border)
            .title(Span::styled(self.title, self.theme.title));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(error_lines),
            Constraint::Length(help_lines),
        ])
        .split(inner_area);

        let list_items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let is_cursor = state.selected() == Some(idx);
                let is_selected = selected_indices.contains(&idx);

                let checkbox = if is_selected { "[x]" } else { "[ ]" };
                let cursor = if is_cursor { ">" } else { " " };

                let style = if !item.enabled {
                    self.theme.disabled
                } else if is_cursor {
                    self.theme.selected
                } else {
                    self.theme.unselected
                };

                ListItem::new(Line::from(Span::styled(
                    format!("{cursor} {checkbox} {}", item.label),
                    style,
                )))
            })
            .collect();

        let list = List::new(list_items).scroll_padding(1);
        frame.render_stateful_widget(list, chunks[0], state);

        // Render error as Paragraph with wrapping so long messages reflow on narrow terminals
        if let Some(err) = error_message {
            let error_para = Paragraph::new(err)
                .style(self.theme.error)
                .wrap(Wrap { trim: true });
            frame.render_widget(error_para, chunks[1]);
        }

        // Render help as Paragraph with wrapping so long text reflows on narrow terminals
        let help = Paragraph::new(help_text)
            .style(self.theme.help)
            .wrap(Wrap { trim: true });
        frame.render_widget(help, chunks[2]);
    }
}

#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(area);

    let horizontal = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1]);

    horizontal[1]
}
