//! Event envelope widget: source badge, event type, timestamps.

use chronicle_core::event::Event;
use egui::Ui;

/// Displays the event envelope: source, type, and timestamps.
pub struct EventEnvelopeWidget<'a> {
    event: &'a Event,
}

impl<'a> EventEnvelopeWidget<'a> {
    pub fn new(event: &'a Event) -> Self {
        Self { event }
    }

    pub fn ui(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let source_color = source_badge_color(self.event.source.as_str());
            let badge = egui::RichText::new(self.event.source.as_str())
                .small()
                .color(egui::Color32::WHITE)
                .background_color(source_color);
            ui.label(badge);

            ui.label(
                egui::RichText::new(self.event.event_type.as_str())
                    .strong()
                    .monospace(),
            );
        });

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Event time:").weak());
            ui.label(
                self.event
                    .event_time
                    .format("%Y-%m-%d %H:%M:%S UTC")
                    .to_string(),
            );
        });

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Ingestion:").weak());
            ui.label(
                self.event
                    .ingestion_time
                    .format("%Y-%m-%d %H:%M:%S UTC")
                    .to_string(),
            );
        });

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Event ID:").weak());
            ui.label(
                egui::RichText::new(self.event.event_id.to_string())
                    .monospace()
                    .small(),
            );
        });
    }
}

fn source_badge_color(source: &str) -> egui::Color32 {
    match source {
        "stripe" => egui::Color32::from_rgb(99, 91, 255),
        "support" => egui::Color32::from_rgb(255, 140, 50),
        "product" => egui::Color32::from_rgb(50, 180, 100),
        "marketing" => egui::Color32::from_rgb(220, 60, 180),
        "billing" => egui::Color32::from_rgb(60, 160, 220),
        _ => egui::Color32::from_rgb(128, 128, 128),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronicle_core::event::EventBuilder;

    #[test]
    fn envelope_does_not_panic_on_minimal_event() {
        let event = EventBuilder::new("o", "s", "t", "e").build();
        let _widget = EventEnvelopeWidget::new(&event);
    }
}
