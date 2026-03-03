//! Entity ref chips: clickable pill badges for entity references.

use chronicle_core::entity_ref::EntityRef;
use egui::Ui;

/// Displays entity refs as clickable chip/pill badges.
pub struct EntityRefChips<'a> {
    refs: &'a [EntityRef],
}

impl<'a> EntityRefChips<'a> {
    pub fn new(refs: &'a [EntityRef]) -> Self {
        Self { refs }
    }

    /// Render the chips. Returns the (`entity_type`, `entity_id`) of any clicked chip.
    pub fn ui(&self, ui: &mut Ui) -> Option<(String, String)> {
        if self.refs.is_empty() {
            ui.label(egui::RichText::new("No entity refs").weak().italics());
            return None;
        }

        let mut clicked = None;

        ui.horizontal_wrapped(|ui| {
            for r in self.refs {
                let label = format!("{}:{}", r.entity_type.as_str(), r.entity_id.as_str());
                let color = entity_type_color(r.entity_type.as_str());

                let chip = egui::Button::new(
                    egui::RichText::new(&label)
                        .small()
                        .color(egui::Color32::WHITE),
                )
                .fill(color)
                .corner_radius(12.0);

                if ui.add(chip).clicked() {
                    clicked = Some((
                        r.entity_type.as_str().to_string(),
                        r.entity_id.as_str().to_string(),
                    ));
                }
            }
        });

        clicked
    }
}

fn entity_type_color(entity_type: &str) -> egui::Color32 {
    match entity_type {
        "customer" => egui::Color32::from_rgb(59, 130, 246),
        "account" => egui::Color32::from_rgb(99, 102, 241),
        "session" => egui::Color32::from_rgb(168, 85, 247),
        "ticket" => egui::Color32::from_rgb(245, 158, 11),
        "user" => egui::Color32::from_rgb(34, 197, 94),
        _ => egui::Color32::from_rgb(107, 114, 128),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chips_handle_empty_refs() {
        let chips = EntityRefChips::new(&[]);
        assert!(chips.refs.is_empty());
    }
}
