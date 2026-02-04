//! Message/alert display component.

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::tui::{TerminalApp, theme::Theme};

/// Style of the message box.
#[derive(Debug, Clone, Copy, Default)]
pub enum MessageStyle {
    /// Informational message (default styling)
    #[default]
    Info,
    /// Error message (red styling)
    Error,
    /// Success message (green styling)
    Success,
}

/// A message display prompt that shows a message and waits for acknowledgment.
pub struct Message<'a> {
    title: &'a str,
    content: &'a str,
    style: MessageStyle,
    theme: Theme,
}

impl<'a> Message<'a> {
    /// Creates a new message prompt.
    #[must_use]
    pub fn new(title: &'a str, content: &'a str) -> Self {
        Self {
            title,
            content,
            style: MessageStyle::default(),
            theme: Theme::default(),
        }
    }

    /// Sets the message style.
    #[must_use]
    pub fn with_style(mut self, style: MessageStyle) -> Self {
        self.style = style;
        self
    }

    /// Displays the message and waits for the user to press any key.
    ///
    /// # Errors
    ///
    /// Returns an error if terminal operations fail.
    pub fn show(self) -> std::io::Result<()> {
        let mut app = TerminalApp::new()?;

        loop {
            app.terminal().draw(|frame| {
                self.render(frame);
            })?;

            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Any key dismisses the message
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    _ => return Ok(()),
                }
            }
        }
    }

    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn render(&self, frame: &mut Frame) {
        let area = centered_rect(50, 7, frame.area());

        frame.render_widget(Clear, area);

        let border_style = match self.style {
            MessageStyle::Info => self.theme.border,
            MessageStyle::Error => self.theme.error,
            MessageStyle::Success => self.theme.success,
        };

        let title_style = match self.style {
            MessageStyle::Info => self.theme.title,
            MessageStyle::Error => self.theme.error,
            MessageStyle::Success => self.theme.success,
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(self.title, title_style));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let chunks =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner_area);

        // Message text
        let content_style = match self.style {
            MessageStyle::Info => self.theme.unselected,
            MessageStyle::Error => self.theme.error,
            MessageStyle::Success => self.theme.success,
        };

        let content = Paragraph::new(self.content)
            .style(content_style)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        frame.render_widget(content, chunks[0]);

        // Help text
        let help_line = Line::from(Span::styled("Press any key to continue", self.theme.help))
            .alignment(Alignment::Center);
        frame.render_widget(help_line, chunks[1]);
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
