// Chronicle widgets define their own color palette for link types, source badges,
// entity types, and JSON syntax highlighting -- not using rerun design_tokens.
#![allow(clippy::disallowed_methods)]

//! Shared viewer widgets and filter state for Chronicle event data.
//!
//! # Widgets
//!
//! - [`EventEnvelopeWidget`] — displays event envelope fields (source, type, time)
//! - [`EntityRefChips`] — renders entity references as colored chips
//! - [`LinksList`] — displays causal links with direction and confidence
//! - [`PayloadJsonWidget`] — syntax-highlighted JSON payload viewer
//!
//! # Filter State
//!
//! [`ChronicleFilterState`] manages 8-dimension filters (entity, source,
//! event type, topic, payload text, payload field, link type, time range)
//! with `discover()`, `matches_path()`, `matches_payload()`, and
//! `build_query()` to produce a [`chronicle_core::query::StructuredQuery`].
//!
//! # Shared Utilities
//!
//! - [`format_event_summary`] — canonical one-line event formatter (used by bridge + widgets)
//! - [`link_type_color`] — color palette matching the time panel link overlay

mod entity_ref_chips;
mod event_envelope;
pub mod filter_state;
mod links_list;
mod payload_json;

pub use entity_ref_chips::EntityRefChips;
pub use event_envelope::EventEnvelopeWidget;
pub use filter_state::{ChronicleFilterState, FieldFilter};
pub use links_list::{LinkDisplay, LinksList};
pub use payload_json::PayloadJsonWidget;

/// Format a concise one-line summary of an event.
///
/// Shows: `[source] event_type | key payload fields`
/// This is the canonical formatter -- also used by `chronicle_rerun_bridge`.
pub fn format_event_summary(event: &chronicle_core::event::Event) -> String {
    let mut parts = vec![format!(
        "[{}] {}",
        event.source.as_str(),
        event.event_type.as_str()
    )];

    if let Some(ref payload) = event.payload {
        if let Some(obj) = payload.as_object() {
            let fields: Vec<String> = obj
                .iter()
                .take(4)
                .map(|(k, v)| {
                    let val = match v {
                        serde_json::Value::String(s) => {
                            if s.len() > 30 {
                                format!("\"{}…\"", &s[..27])
                            } else {
                                format!("\"{s}\"")
                            }
                        }
                        other => other.to_string(),
                    };
                    format!("{k}={val}")
                })
                .collect();

            if !fields.is_empty() {
                parts.push(fields.join(", "));
            }
        }
    }

    parts.join(" | ")
}

/// Map link types to distinct colors (matches `re_time_panel` `link_overlay`).
pub fn link_type_color(link_type: &str, confidence: f32) -> egui::Color32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let alpha = (confidence * 200.0 + 55.0) as u8;
    match link_type {
        "caused_by" => egui::Color32::from_rgba_unmultiplied(255, 80, 80, alpha),
        "led_to" => egui::Color32::from_rgba_unmultiplied(255, 160, 60, alpha),
        "triggered" => egui::Color32::from_rgba_unmultiplied(255, 220, 50, alpha),
        "related_to" => egui::Color32::from_rgba_unmultiplied(80, 160, 255, alpha),
        "campaign_conversion" => egui::Color32::from_rgba_unmultiplied(80, 220, 120, alpha),
        _ => egui::Color32::from_rgba_unmultiplied(180, 180, 180, alpha),
    }
}
