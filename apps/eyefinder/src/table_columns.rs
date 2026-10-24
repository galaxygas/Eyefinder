//! Stable resize origins for the asset tables.
//!
//! egui_extras 0.36 `TableState::load` recomputes a non-resizable remainder on each
//! frame, while `Table::body` resizes relative to the current column's left edge.
//! A remainder before a resizable column therefore moves that edge while dragging.
//! Keep it last. Source: https://github.com/emilk/egui/blob/0.36.0/crates/egui_extras/src/table.rs

use egui_extras::{Column, TableBuilder};

pub(crate) fn audio(table: TableBuilder<'_>) -> TableBuilder<'_> {
    table
        .column(Column::exact(34.0))
        .column(Column::initial(320.0).at_least(160.0).clip(true).resizable(true))
        .column(Column::initial(160.0).at_least(100.0).clip(true).resizable(true))
        .column(Column::initial(110.0).at_least(80.0).clip(true).resizable(true))
        .column(Column::initial(70.0).at_least(60.0))
        .column(Column::remainder().at_least(112.0).clip(true))
}

pub(crate) fn textures(table: TableBuilder<'_>) -> TableBuilder<'_> {
    table
        .column(Column::exact(40.0))
        .column(Column::initial(240.0).at_least(140.0).clip(true).resizable(true))
        .column(Column::initial(90.0).at_least(70.0))
        .column(Column::initial(150.0).at_least(90.0).clip(true).resizable(true))
        .column(Column::initial(46.0))
        .column(Column::initial(200.0).at_least(100.0).clip(true).resizable(true))
        .column(Column::remainder().at_least(112.0).clip(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    fn frame(ctx: &Context, texture: bool, events: Vec<Event>) -> Vec<Rect> {
        let mut cells = Vec::new();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 400.0))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            cells.clear();
            egui::CentralPanel::default().show(ui, |ui| {
                let table = TableBuilder::new(ui);
                let table = if texture { textures(table) } else { audio(table) };
                table
                    .header(26.0, |mut header| {
                        for _ in 0..if texture { 7 } else { 6 } {
                            header.col(|ui| {
                                cells.push(ui.max_rect());
                            });
                        }
                    })
                    .body(|mut body| {
                        body.row(32.0, |mut row| {
                            for _ in 0..if texture { 7 } else { 6 } {
                                row.col(|ui| {
                                    ui.label("item");
                                });
                            }
                        });
                    });
            });
        });
        output.textures_delta.clear();
        cells
    }

    #[test]
    fn resize_tracks_pointer_without_drift_in_both_asset_tables() {
        for (texture, columns) in [(false, vec![1, 2, 3]), (true, vec![1, 3, 5])] {
            for column in columns {
                for delta in [-20.0, 20.0] {
                    let ctx = Context::default();
                    frame(&ctx, texture, vec![]);
                    let before = frame(&ctx, texture, vec![]);
                    let edge = (before[column].right() + before[column + 1].left()) * 0.5;
                    let start = Pos2::new(edge, before[column].center().y);
                    frame(&ctx, texture, vec![Event::PointerMoved(start)]);
                    frame(
                        &ctx,
                        texture,
                        vec![Event::PointerButton {
                            pos: start,
                            button: PointerButton::Primary,
                            pressed: true,
                            modifiers: Modifiers::NONE,
                        }],
                    );
                    let end = start + Vec2::new(delta, 0.0);
                    frame(&ctx, texture, vec![Event::PointerMoved(end)]);
                    for _ in 0..5 {
                        let after = frame(&ctx, texture, vec![]);
                        assert!((after[column].left() - before[column].left()).abs() < 0.1);
                        assert!(
                            (after[column].right() - before[column].right() - delta).abs() < 0.1,
                            "texture={texture}, column={column}, delta={delta}, before={:?}, after={:?}",
                            before[column],
                            after[column]
                        );
                    }
                    frame(
                        &ctx,
                        texture,
                        vec![Event::PointerButton {
                            pos: end,
                            button: PointerButton::Primary,
                            pressed: false,
                            modifiers: Modifiers::NONE,
                        }],
                    );
                }
            }
        }
    }
}
