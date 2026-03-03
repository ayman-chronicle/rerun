#![allow(clippy::disallowed_methods)]

use eframe::egui;

use chronicle_core::entity_ref::EntityRef;
use chronicle_core::event::{Event, EventBuilder};
use chronicle_core::ids::*;
use chronicle_core::link::EventLink;
use chronicle_viewer_common::*;
use chrono::{Duration, Utc};

struct DemoApp {
    events: Vec<Event>,
    entity_refs: Vec<Vec<EntityRef>>,
    links: Vec<EventLink>,
    selected_event_idx: Option<usize>,
    event_positions: Vec<(usize, f32)>,
}

impl DemoApp {
    fn new() -> Self {
        let now = Utc::now();
        let mut events = Vec::new();

        #[allow(deprecated)]
        let day = |n: i64| Duration::days(n);

        for i in 0i64..5 {
            events.push(
                EventBuilder::new("demo", "stripe", "payments", "payment_intent.succeeded")
                    .entity("customer", "cust_001")
                    .payload(serde_json::json!({"amount": 4999 + i * 1000, "currency": "usd"}))
                    .event_time(now - day(80 - i * 15))
                    .build(),
            );
        }

        for i in 0i64..5 {
            events.push(
                EventBuilder::new("demo", "support", "tickets", "ticket.created")
                    .entity("customer", "cust_001")
                    .entity("ticket", format!("tkt_{i}"))
                    .payload(
                        serde_json::json!({"subject": format!("Issue #{i}"), "priority": "high"}),
                    )
                    .event_time(now - day(70 - i * 12))
                    .build(),
            );
        }

        for i in 0i64..5 {
            events.push(
                EventBuilder::new("demo", "product", "usage", "page.viewed")
                    .entity("customer", "cust_001")
                    .payload(
                        serde_json::json!({"url": format!("/page_{i}"), "duration_ms": 3000 + i * 1000}),
                    )
                    .event_time(now - day(60 - i * 10))
                    .build(),
            );
        }

        for i in 0i64..5 {
            events.push(
                EventBuilder::new("demo", "marketing", "campaigns", "campaign.sent")
                    .entity("customer", "cust_001")
                    .payload(
                        serde_json::json!({"campaign": format!("camp_{i}"), "channel": "email"}),
                    )
                    .event_time(now - day(85 - i * 18))
                    .build(),
            );
        }

        for i in 0i64..5 {
            events.push(
                EventBuilder::new("demo", "billing", "invoices", "invoice.created")
                    .entity("customer", "cust_001")
                    .payload(
                        serde_json::json!({"amount": 4999, "period": format!("2025-{:02}", i + 1)}),
                    )
                    .event_time(now - day(75 - i * 14))
                    .build(),
            );
        }

        let entity_refs: Vec<Vec<EntityRef>> = events
            .iter()
            .map(|e| e.materialize_entity_refs("demo"))
            .collect();

        let links = vec![
            EventLink {
                link_id: LinkId::new(),
                source_event_id: events[2].event_id,
                target_event_id: events[5].event_id,
                link_type: "caused_by".to_string(),
                confidence: Confidence::new(0.85).unwrap(),
                reasoning: Some("Payment issue led to support ticket".to_string()),
                created_by: "demo".to_string(),
                created_at: Utc::now(),
            },
            EventLink {
                link_id: LinkId::new(),
                source_event_id: events[6].event_id,
                target_event_id: events[22].event_id,
                link_type: "led_to".to_string(),
                confidence: Confidence::new(0.9).unwrap(),
                reasoning: Some("Ticket escalation triggered invoice review".to_string()),
                created_by: "demo".to_string(),
                created_at: Utc::now(),
            },
            EventLink {
                link_id: LinkId::new(),
                source_event_id: events[15].event_id,
                target_event_id: events[10].event_id,
                link_type: "campaign_conversion".to_string(),
                confidence: Confidence::new(0.75).unwrap(),
                reasoning: Some("Campaign click led to product page view".to_string()),
                created_by: "demo".to_string(),
                created_at: Utc::now(),
            },
            EventLink {
                link_id: LinkId::new(),
                source_event_id: events[7].event_id,
                target_event_id: events[4].event_id,
                link_type: "triggered".to_string(),
                confidence: Confidence::new(0.7).unwrap(),
                reasoning: Some("Support resolution triggered refund".to_string()),
                created_by: "demo".to_string(),
                created_at: Utc::now(),
            },
        ];

        let event_positions: Vec<(usize, f32)> = events
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let row = i / 5;
                let col = i % 5;
                let x_frac = 0.1 + col as f32 * 0.18;
                (row, x_frac)
            })
            .collect();

        Self {
            events,
            entity_refs,
            links,
            selected_event_idx: None,
            event_positions,
        }
    }

    fn source_for_row(row: usize) -> (&'static str, egui::Color32) {
        match row {
            0 => ("stripe/payment", egui::Color32::from_rgb(99, 91, 255)),
            1 => ("support/ticket", egui::Color32::from_rgb(255, 140, 50)),
            2 => ("product/page", egui::Color32::from_rgb(50, 180, 100)),
            3 => ("marketing/campaign", egui::Color32::from_rgb(220, 60, 180)),
            4 => ("billing/invoice", egui::Color32::from_rgb(60, 160, 220)),
            _ => ("unknown", egui::Color32::GRAY),
        }
    }
}

impl eframe::App for DemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ---------- Right panel: detail widgets ----------
        egui::SidePanel::right("detail_panel")
            .min_width(300.0)
            .default_width(350.0)
            .show(ctx, |ui| {
                ui.heading("Event Detail");
                ui.separator();

                if let Some(idx) = self.selected_event_idx {
                    let event = &self.events[idx];
                    let refs = &self.entity_refs[idx];

                    ui.collapsing("Envelope", |ui| {
                        EventEnvelopeWidget::new(event).ui(ui);
                    });

                    ui.add_space(8.0);

                    ui.collapsing("Entity Refs", |ui| {
                        EntityRefChips::new(refs).ui(ui);
                    });

                    ui.add_space(8.0);

                    let event_links: Vec<LinkDisplay> = self.links
                        .iter()
                        .filter(|l| {
                            l.source_event_id == event.event_id
                                || l.target_event_id == event.event_id
                        })
                        .map(|l| {
                            let is_outgoing = l.source_event_id == event.event_id;
                            let other_id = if is_outgoing {
                                l.target_event_id
                            } else {
                                l.source_event_id
                            };
                            let other_event =
                                self.events.iter().find(|e| e.event_id == other_id);
                            let summary = other_event
                                .map(format_event_summary)
                                .unwrap_or_else(|| "Unknown event".to_string());
                            LinkDisplay {
                                link: l.clone(),
                                other_event_summary: summary,
                                is_outgoing,
                            }
                        })
                        .collect();

                    ui.collapsing("Links", |ui| {
                        if let Some(target_id) = LinksList::new(&event_links).ui(ui) {
                            if let Some(pos) = self.events.iter().position(|e| {
                                e.event_id.to_string() == target_id
                            }) {
                                self.selected_event_idx = Some(pos);
                            }
                        }
                    });

                    ui.add_space(8.0);

                    ui.collapsing("Payload", |ui| {
                        PayloadJsonWidget::new(event.payload.as_ref()).ui(ui);
                    });
                } else {
                    ui.label(
                        egui::RichText::new("Click an event dot to inspect it")
                            .weak()
                            .italics(),
                    );
                }
            });

        // ---------- Central panel: swimlane timeline ----------
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Chronicle Timeline");
            ui.label("Click event dots to see details. Arcs show causal links.");
            ui.separator();

            let available = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(available, egui::Sense::click());
            let painter = ui.painter_at(available);

            let row_height = available.height() / 6.0;
            let margin_left = 150.0;
            let timeline_width = available.width() - margin_left - 20.0;

            // Compute Y centers for each swimlane row
            let row_y: Vec<f32> = (0..5)
                .map(|row| available.top() + row_height * (row as f32 + 0.5) + 30.0)
                .collect();

            // Draw swimlane rows
            for row in 0..5 {
                let (name, color) = Self::source_for_row(row);
                let y = row_y[row];

                let row_rect = egui::Rect::from_min_size(
                    egui::pos2(available.left(), y - row_height * 0.35),
                    egui::vec2(available.width(), row_height * 0.7),
                );
                painter.rect_filled(row_rect, 0.0, color.gamma_multiply(0.08));

                painter.text(
                    egui::pos2(available.left() + 10.0, y),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(13.0),
                    color,
                );

                painter.hline(
                    (available.left() + margin_left)..=(available.right() - 20.0),
                    y,
                    egui::Stroke::new(1.0, color.gamma_multiply(0.3)),
                );
            }

            // Draw event dots
            for (i, &(row, x_frac)) in self.event_positions.iter().enumerate() {
                let (_, color) = Self::source_for_row(row);
                let x = available.left() + margin_left + x_frac * timeline_width;
                let y = row_y[row];

                let is_selected = self.selected_event_idx == Some(i);
                let radius = if is_selected { 7.0 } else { 5.0 };
                let dot_color = if is_selected {
                    egui::Color32::WHITE
                } else {
                    color
                };

                painter.circle_filled(egui::pos2(x, y), radius, dot_color);
                if is_selected {
                    painter.circle_stroke(
                        egui::pos2(x, y),
                        radius + 2.0,
                        egui::Stroke::new(2.0, color),
                    );
                }
            }

            // Click: select nearest dot
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    for (i, &(row, x_frac)) in self.event_positions.iter().enumerate() {
                        let x = available.left() + margin_left + x_frac * timeline_width;
                        let y = row_y[row];
                        if (pos - egui::pos2(x, y)).length() < 10.0 {
                            self.selected_event_idx = Some(i);
                            break;
                        }
                    }
                }
            }

            // Hover: glow ring + tooltip
            let mut hovered_idx = None;
            if let Some(pos) = response.hover_pos() {
                for (i, &(row, x_frac)) in self.event_positions.iter().enumerate() {
                    let x = available.left() + margin_left + x_frac * timeline_width;
                    let y = row_y[row];
                    if (pos - egui::pos2(x, y)).length() < 10.0 {
                        hovered_idx = Some(i);
                        let radius = if self.selected_event_idx == Some(i) {
                            7.0
                        } else {
                            5.0
                        };
                        painter.circle_stroke(
                            egui::pos2(x, y),
                            radius + 3.0,
                            egui::Stroke::new(1.0, egui::Color32::WHITE),
                        );
                        break;
                    }
                }
            }

            if let Some(idx) = hovered_idx {
                let summary = format_event_summary(&self.events[idx]);
                #[allow(deprecated)]
                egui::show_tooltip_at_pointer(
                    ui.ctx(),
                    ui.layer_id(),
                    egui::Id::new("event_tooltip"),
                    |ui| {
                        ui.label(summary);
                    },
                );
            }

            // Draw link arcs (Bezier curves between connected events)
            for link in &self.links {
                let src_idx = self
                    .events
                    .iter()
                    .position(|e| e.event_id == link.source_event_id);
                let tgt_idx = self
                    .events
                    .iter()
                    .position(|e| e.event_id == link.target_event_id);

                if let (Some(si), Some(ti)) = (src_idx, tgt_idx) {
                    let (src_row, src_x_frac) = self.event_positions[si];
                    let (tgt_row, tgt_x_frac) = self.event_positions[ti];

                    let x0 = available.left() + margin_left + src_x_frac * timeline_width;
                    let y0 = row_y[src_row];
                    let x1 = available.left() + margin_left + tgt_x_frac * timeline_width;
                    let y1 = row_y[tgt_row];

                    let is_active = self
                        .selected_event_idx
                        .map_or(true, |sel| sel == si || sel == ti);
                    let conf = if is_active {
                        link.confidence.value()
                    } else {
                        link.confidence.value() * 0.2
                    };

                    let color = link_type_color(&link.link_type, conf);
                    let stroke_width = if is_active { 2.5 } else { 1.0 };
                    let stroke = egui::Stroke::new(stroke_width, color);

                    // Control point above the midpoint for the arc
                    let mid_x = (x0 + x1) / 2.0;
                    let arc_height = ((y0 - y1).abs() * 0.3).max(25.0);
                    let control_y = y0.min(y1) - arc_height;

                    painter.add(egui::epaint::CubicBezierShape {
                        points: [
                            egui::pos2(x0, y0),
                            egui::pos2(mid_x, control_y),
                            egui::pos2(mid_x, control_y),
                            egui::pos2(x1, y1),
                        ],
                        closed: false,
                        fill: egui::Color32::TRANSPARENT,
                        stroke: stroke.into(),
                    });

                    if is_active {
                        // Source dot
                        painter.circle_filled(egui::pos2(x0, y0), 4.0, color);

                        // Arrow at target
                        let size = 6.0_f32;
                        let dir = if x1 > x0 { -1.0_f32 } else { 1.0 };
                        let p1 = egui::pos2(x1 + dir * size, y1 - size * 0.5);
                        let p2 = egui::pos2(x1 + dir * size, y1 + size * 0.5);
                        painter.add(egui::Shape::convex_polygon(
                            vec![egui::pos2(x1, y1), p1, p2],
                            color,
                            egui::Stroke::NONE,
                        ));
                    }
                }
            }
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 700.0])
            .with_title("Chronicle Viewer - Detail Widget Demo"),
        ..Default::default()
    };

    eframe::run_native(
        "Chronicle Detail Demo",
        options,
        Box::new(|_cc| Ok(Box::new(DemoApp::new()))),
    )
}
