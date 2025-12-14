//! UI rendering for Sigilforge TUI.

use crate::app::{App, TokenStatus};
use anyhow::Result;
use fusabi_tui_core::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
};
use fusabi_tui_widgets::{
    block::{Block, Title, TitleAlignment},
    borders::{BorderType, Borders},
    list::{List, ListItem, ListState},
    paragraph::{Alignment, Paragraph, Wrap},
    text::{Line, Span, Text},
    widget::{StatefulWidget, Widget},
};

// Sigilforge color theme
const COLOR_PRIMARY: Color = Color::Cyan;
const COLOR_SUCCESS: Color = Color::Green;
const COLOR_WARNING: Color = Color::Yellow;
const COLOR_ERROR: Color = Color::Red;
const COLOR_TEXT: Color = Color::White;
const COLOR_DIM: Color = Color::DarkGray;

/// Render the entire UI
pub fn render(app: &App) -> Result<Buffer> {
    // Get terminal size (default to 80x24 if we can't detect)
    let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
    let area = Rect::new(0, 0, width, height);

    let mut buffer = Buffer::new(area);

    // Create main layout: title | content | status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(&[
            Constraint::Length(3),      // Title
            Constraint::Fill(1),        // Content
            Constraint::Length(3),      // Status bar
        ])
        .split(area);

    // Render title
    render_title(chunks[0], &mut buffer);

    // Render content (accounts list and details)
    render_content(app, chunks[1], &mut buffer);

    // Render status bar
    render_status_bar(app, chunks[2], &mut buffer);

    Ok(buffer)
}

/// Render the title bar
fn render_title(area: Rect, buffer: &mut Buffer) {
    let title_block = Block::default()
        .title(
            Title::new("Sigilforge OAuth Token Manager")
                .alignment(TitleAlignment::Center)
                .style(
                    Style::default()
                        .fg(COLOR_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(COLOR_PRIMARY));

    title_block.render(area, buffer);
}

/// Render the main content area
fn render_content(app: &App, area: Rect, buffer: &mut Buffer) {
    // Split into three columns: accounts list | details | help
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(&[
            Constraint::Percentage(40),
            Constraint::Percentage(40),
            Constraint::Percentage(20),
        ])
        .split(area);

    render_accounts_list(app, chunks[0], buffer);
    render_account_details(app, chunks[1], buffer);
    render_help(chunks[2], buffer);
}

/// Render the accounts list
fn render_accounts_list(app: &App, area: Rect, buffer: &mut Buffer) {
    let list_block = Block::default()
        .title("OAuth Accounts")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_TEXT));

    if app.accounts.is_empty() {
        // Show empty message
        let empty_text = if app.daemon_available {
            "No OAuth accounts configured.\n\nUse the sigilforge CLI to add accounts."
        } else {
            "Sigilforge daemon is not available.\n\nPlease start the daemon:\n  sigilforged"
        };

        let paragraph = Paragraph::new(Text::from(empty_text))
            .block(list_block)
            .style(Style::default().fg(COLOR_DIM))
            .alignment(Alignment::Center)
            .wrap(Wrap::WordWrap);

        paragraph.render(area, buffer);
    } else {
        // Create list items
        let items: Vec<ListItem> = app
            .accounts
            .iter()
            .map(|account| {
                let status_color = match account.status {
                    TokenStatus::Valid => COLOR_SUCCESS,
                    TokenStatus::ExpiringSoon => COLOR_WARNING,
                    TokenStatus::Expired => COLOR_ERROR,
                    TokenStatus::Unknown => COLOR_DIM,
                };

                let line = Line::from(vec![
                    Span::styled(
                        format!("{:12}", account.service),
                        Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("[{}]", account.status_text()),
                        Style::default().fg(status_color),
                    ),
                    Span::raw("  "),
                    Span::styled(&account.account, Style::default().fg(COLOR_DIM)),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(list_block)
            .highlight_style(
                Style::default()
                    .bg(COLOR_PRIMARY)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            );

        let mut state = ListState::default();
        state.select(Some(app.selected));

        list.render(area, buffer, &mut state);
    }
}

/// Render account details panel
fn render_account_details(app: &App, area: Rect, buffer: &mut Buffer) {
    let details_block = Block::default()
        .title("Account Details")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_TEXT));

    if let Some(account) = app.selected_account() {
        // Pre-compute strings that need to be owned
        let expiry_text = account.expiry_display();

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Service: ", Style::default().fg(COLOR_DIM)),
                Span::styled(
                    &account.service,
                    Style::default()
                        .fg(COLOR_TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Account: ", Style::default().fg(COLOR_DIM)),
                Span::styled(&account.account, Style::default().fg(COLOR_TEXT)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(COLOR_DIM)),
                Span::styled(
                    account.status_text(),
                    Style::default().fg(match account.status {
                        TokenStatus::Valid => COLOR_SUCCESS,
                        TokenStatus::ExpiringSoon => COLOR_WARNING,
                        TokenStatus::Expired => COLOR_ERROR,
                        TokenStatus::Unknown => COLOR_DIM,
                    }),
                ),
            ]),
            Line::from(vec![
                Span::styled("Expiry: ", Style::default().fg(COLOR_DIM)),
                Span::styled(expiry_text, Style::default().fg(COLOR_TEXT)),
            ]),
            Line::from(""),
        ];

        // Add scopes
        if !account.scopes.is_empty() {
            lines.push(Line::from(Span::styled(
                "Scopes:",
                Style::default().fg(COLOR_DIM),
            )));
            for scope in &account.scopes {
                lines.push(Line::from(format!("  - {}", scope)));
            }
            lines.push(Line::from(""));
        }

        // Add timestamps
        lines.push(Line::from(vec![
            Span::styled("Created: ", Style::default().fg(COLOR_DIM)),
            Span::styled(&account.created_at, Style::default().fg(COLOR_TEXT)),
        ]));

        if let Some(last_used) = &account.last_used {
            lines.push(Line::from(vec![
                Span::styled("Last used: ", Style::default().fg(COLOR_DIM)),
                Span::styled(last_used, Style::default().fg(COLOR_TEXT)),
            ]));
        }

        let paragraph = Paragraph::new(Text::from(lines))
            .block(details_block)
            .wrap(Wrap::WordWrap);

        paragraph.render(area, buffer);
    } else {
        let empty_text = "No account selected";
        let paragraph = Paragraph::new(Text::from(empty_text))
            .block(details_block)
            .style(Style::default().fg(COLOR_DIM))
            .alignment(Alignment::Center);

        paragraph.render(area, buffer);
    }
}

/// Render help panel
fn render_help(area: Rect, buffer: &mut Buffer) {
    let help_block = Block::default()
        .title("Keyboard")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_TEXT));

    let help_text = vec![
        Line::from(Span::styled(
            "Navigation:",
            Style::default()
                .fg(COLOR_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("j/↓  - Next"),
        Line::from("k/↑  - Previous"),
        Line::from("g    - First"),
        Line::from("G    - Last"),
        Line::from(""),
        Line::from(Span::styled(
            "Actions:",
            Style::default()
                .fg(COLOR_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("r    - Refresh"),
        Line::from("a    - Refresh all"),
        Line::from("q    - Quit"),
    ];

    let paragraph = Paragraph::new(Text::from(help_text))
        .block(help_block)
        .style(Style::default().fg(COLOR_TEXT));

    paragraph.render(area, buffer);
}

/// Render the status bar
fn render_status_bar(app: &App, area: Rect, buffer: &mut Buffer) {
    let status_style = if app.daemon_available {
        Style::default().fg(COLOR_SUCCESS)
    } else {
        Style::default().fg(COLOR_ERROR)
    };

    let status_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(status_style);

    let daemon_status = if app.daemon_available {
        Span::styled(" Connected ", status_style.add_modifier(Modifier::BOLD))
    } else {
        Span::styled(
            " Daemon Unavailable ",
            Style::default()
                .fg(COLOR_ERROR)
                .add_modifier(Modifier::BOLD),
        )
    };

    let status_line = Line::from(vec![
        daemon_status,
        Span::raw(" | "),
        Span::styled(&app.status_message, Style::default().fg(COLOR_TEXT)),
    ]);

    let paragraph = Paragraph::new(Text::from(vec![status_line]))
        .block(status_block)
        .alignment(Alignment::Left);

    paragraph.render(area, buffer);
}
