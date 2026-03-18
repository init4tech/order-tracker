use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Cell, Paragraph, Row, Table},
};
use signet_tracker::{MaybeBool, OrderStatus};

const PENDING_COLOR: Color = Color::Yellow;
const FILLED_COLOR: Color = Color::Green;
const EXPIRED_COLOR: Color = Color::Red;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [list_area, detail_area, help_area] =
        Layout::vertical([Constraint::Percentage(40), Constraint::Fill(1), Constraint::Length(1)])
            .areas(frame.area());

    draw_order_list(frame, app, list_area);
    draw_details(frame, app, detail_area);
    draw_help_bar(frame, app, help_area);
}

fn draw_order_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let (pending, filled, expired) = app.counts();
    let title = format!(" Orders — {pending} pending │ {filled} filled │ {expired} expired ");

    let header = Row::new(["Hash", "Status", "Info"])
        .style(Style::default().fg(Color::DarkGray))
        .bottom_margin(1);

    let rows: Vec<Row> = app
        .orders()
        .iter()
        .map(|order| {
            let hash = truncate_hash(&format!("{}", order.order_hash()));
            let (status_str, color) = status_display(order);
            let info = info_summary(order);
            Row::new([
                Cell::from(hash),
                Cell::from(status_str).style(Style::default().fg(color)),
                Cell::from(info),
            ])
        })
        .collect();

    let widths = [Constraint::Length(17), Constraint::Length(9), Constraint::Fill(1)];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::bordered().title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_details(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" Details ");

    let Some(order) = app.selected_order() else {
        let paragraph = Paragraph::new("No order selected")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(paragraph, area);
        return;
    };

    let label = Style::default().fg(Color::DarkGray);
    let accent = Style::default().fg(Color::Cyan);
    let mut lines = Vec::new();

    // Order hash + status header
    lines.push(Line::from(vec![
        Span::styled("Order: ", label),
        Span::styled(format!("{}", order.order_hash()), accent),
    ]));
    let (status_str, color) = status_display(order);
    lines.push(Line::from(vec![
        Span::styled("Status: ", label),
        Span::styled(status_str, Style::default().fg(color).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::default());

    match order {
        OrderStatus::Pending { diagnostics, .. } | OrderStatus::Expired { diagnostics, .. } => {
            lines.push(Line::from(vec![
                Span::styled("In cache: ", label),
                Span::styled(
                    maybe_bool_icon(diagnostics.is_in_cache),
                    maybe_bool_style(diagnostics.is_in_cache),
                ),
            ]));

            lines.push(Line::from(vec![
                Span::styled("Deadline: ", label),
                Span::raw(diagnostics.deadline_check.deadline.to_string()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Expires in: ", label),
                Span::raw(diagnostics.deadline_check.expires_in.to_string()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Checked at: ", label),
                Span::raw(diagnostics.deadline_check.checked_at.to_string()),
            ]));

            lines.push(Line::default());

            lines.push(Line::from(vec![
                Span::styled("Allowances: ", label),
                Span::styled(
                    maybe_bool_icon(diagnostics.allowance_checks.all_sufficient),
                    maybe_bool_style(diagnostics.allowance_checks.all_sufficient),
                ),
            ]));
            for check in &diagnostics.allowance_checks.checks {
                let (icon, icon_color) = bool_icon(check.sufficient);
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(icon, Style::default().fg(icon_color)),
                    Span::raw(format!(
                        " {}: {} / {}",
                        check.token_symbol, check.allowance, check.required
                    )),
                ]));
            }

            lines.push(Line::default());

            lines.push(Line::from(vec![
                Span::styled("Balances: ", label),
                Span::styled(
                    maybe_bool_icon(diagnostics.balance_checks.all_sufficient),
                    maybe_bool_style(diagnostics.balance_checks.all_sufficient),
                ),
            ]));
            for check in &diagnostics.balance_checks.checks {
                let (icon, icon_color) = bool_icon(check.sufficient);
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(icon, Style::default().fg(icon_color)),
                    Span::raw(format!(
                        " {}: {} / {}",
                        check.token_symbol, check.balance, check.required
                    )),
                ]));
            }
        }
        OrderStatus::Filled { fill_info, .. } => {
            if let Some(info) = fill_info {
                lines.push(Line::from(vec![
                    Span::styled("Block: ", label),
                    Span::raw(info.block_number.to_string()),
                ]));

                if let Some(tx) = &info.rollup_initiation_tx {
                    lines.push(Line::from(vec![
                        Span::styled("Initiation tx: ", label),
                        Span::styled(format!("{tx}"), accent),
                    ]));
                }

                if let Some(fill_tx) = &info.fill_tx {
                    lines.push(Line::from(vec![
                        Span::styled(format!("Fill tx ({}): ", fill_tx.chain), label),
                        Span::styled(format!("{}", fill_tx.tx_hash), accent),
                    ]));
                }

                if !info.outputs.is_empty() {
                    lines.push(Line::default());
                    lines.push(Line::from(Span::styled("Outputs:", label)));
                    for output in &info.outputs {
                        lines.push(Line::from(format!(
                            "  {} {} → {} ({})",
                            output.token_symbol,
                            output.amount,
                            truncate_hash(&format!("{}", output.recipient)),
                            output.chain,
                        )));
                    }
                }
            } else {
                lines.push(Line::from(Span::styled("Fill info not available", label)));
            }
        }
    }

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn draw_help_bar(frame: &mut Frame, app: &App, area: Rect) {
    let (status_text, status_color) =
        if app.connected { ("connected", Color::Green) } else { ("disconnected", Color::Red) };

    let line = Line::from(vec![
        Span::styled(" ↑/↓ ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("navigate  ", Style::default().fg(Color::DarkGray)),
        Span::styled("q ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("quit", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(status_text, Style::default().fg(status_color)),
    ]);

    frame.render_widget(Paragraph::new(line), area);
}

fn truncate_hash(hash: &str) -> String {
    if hash.len() > 14 {
        format!("{}…{}", &hash[..10], &hash[hash.len() - 4..])
    } else {
        hash.to_string()
    }
}

fn status_display(order: &OrderStatus) -> (&'static str, Color) {
    match order {
        OrderStatus::Pending { .. } => ("PENDING", PENDING_COLOR),
        OrderStatus::Filled { .. } => ("FILLED", FILLED_COLOR),
        OrderStatus::Expired { .. } => ("EXPIRED", EXPIRED_COLOR),
    }
}

fn info_summary(order: &OrderStatus) -> String {
    match order {
        OrderStatus::Pending { diagnostics, .. } | OrderStatus::Expired { diagnostics, .. } => {
            format!(
                "{}  cache:{} bal:{} allow:{}",
                diagnostics.deadline_check.expires_in,
                maybe_bool_icon(diagnostics.is_in_cache),
                maybe_bool_icon(diagnostics.balance_checks.all_sufficient),
                maybe_bool_icon(diagnostics.allowance_checks.all_sufficient),
            )
        }
        OrderStatus::Filled { fill_info, .. } => match fill_info {
            Some(info) => format!("block {}", info.block_number),
            None => "fill info unavailable".to_string(),
        },
    }
}

fn maybe_bool_icon(value: MaybeBool) -> &'static str {
    match value {
        MaybeBool::True => "✓",
        MaybeBool::False => "✗",
        MaybeBool::Unknown => "?",
    }
}

fn maybe_bool_style(value: MaybeBool) -> Style {
    match value {
        MaybeBool::True => Style::default().fg(Color::Green),
        MaybeBool::False => Style::default().fg(Color::Red),
        MaybeBool::Unknown => Style::default().fg(Color::Yellow),
    }
}

fn bool_icon(value: bool) -> (&'static str, Color) {
    if value { ("✓", Color::Green) } else { ("✗", Color::Red) }
}
