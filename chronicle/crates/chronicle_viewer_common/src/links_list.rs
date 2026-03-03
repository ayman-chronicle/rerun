//! Links list: displays event links with direction, type, and confidence.

use chronicle_core::link::EventLink;
use egui::Ui;

/// A link ready for display (with resolved event summary for the other end).
pub struct LinkDisplay {
    pub link: EventLink,
    /// Summary of the event at the other end of the link.
    pub other_event_summary: String,
    /// Whether this event is the source (outgoing) or target (incoming).
    pub is_outgoing: bool,
}

/// Displays a list of event links with direction arrows and confidence bars.
pub struct LinksList<'a> {
    links: &'a [LinkDisplay],
}

impl<'a> LinksList<'a> {
    pub fn new(links: &'a [LinkDisplay]) -> Self {
        Self { links }
    }

    /// Render the links list. Returns the `event_id` of any clicked link.
    pub fn ui(&self, ui: &mut Ui) -> Option<String> {
        if self.links.is_empty() {
            ui.label(egui::RichText::new("No links").weak().italics());
            return None;
        }

        let mut clicked = None;

        for display in self.links {
            let link = &display.link;
            let color = crate::link_type_color(&link.link_type, link.confidence.value());

            let arrow = if display.is_outgoing { "→" } else { "←" };
            let direction = if display.is_outgoing {
                "outgoing"
            } else {
                "incoming"
            };

            let label = format!(
                "{arrow} {} ({direction}, {:.0}%) {summary}",
                link.link_type,
                link.confidence.value() * 100.0,
                summary = display.other_event_summary,
            );

            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, color);

                if ui
                    .add(
                        egui::Label::new(egui::RichText::new(&label).small())
                            .sense(egui::Sense::click()),
                    )
                    .clicked()
                {
                    let target_id = if display.is_outgoing {
                        link.target_event_id.to_string()
                    } else {
                        link.source_event_id.to_string()
                    };
                    clicked = Some(target_id);
                }
            });

            let bar_width = 100.0;
            let (bar_rect, _) =
                ui.allocate_exact_size(egui::vec2(bar_width, 4.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(bar_rect, 2.0, egui::Color32::from_gray(60));
            let filled_rect = egui::Rect::from_min_size(
                bar_rect.min,
                egui::vec2(bar_width * link.confidence.value(), 4.0),
            );
            ui.painter().rect_filled(filled_rect, 2.0, color);

            if let Some(ref reasoning) = link.reasoning {
                ui.label(egui::RichText::new(reasoning).weak().italics().small());
            }

            ui.add_space(4.0);
        }

        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_list_handles_empty() {
        let list = LinksList::new(&[]);
        assert!(list.links.is_empty());
    }
}
