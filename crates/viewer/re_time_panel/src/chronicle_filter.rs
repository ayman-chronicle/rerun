//! Tokenized filter bar for Chronicle events.
//!
//! Implements the "Add filter → field / operator / value → chip" pattern.
//! Each active filter renders as a removable chip; "+ Add filter" opens
//! a field picker to create new tokens. Self-contained in the Rerun viewer
//! workspace (no Chronicle crate dependencies).

use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------
// Filter token model
// ---------------------------------------------------------------------------

/// A filter field the user can add.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FilterField {
    Source,
    EventType,
    EntityType,
    EntityId,
    PayloadText,
}

impl FilterField {
    pub const ALL: &[Self] = &[
        Self::Source,
        Self::EventType,
        Self::EntityType,
        Self::EntityId,
        Self::PayloadText,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Source => "Source",
            Self::EventType => "Event Type",
            Self::EntityType => "Entity Type",
            Self::EntityId => "Entity ID",
            Self::PayloadText => "Payload contains",
        }
    }

    fn has_suggestions(self) -> bool {
        matches!(self, Self::Source | Self::EventType | Self::EntityType | Self::EntityId)
    }
}

/// An active filter token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterToken {
    pub field: FilterField,
    pub value: String,
}

impl FilterToken {
    fn chip_label(&self) -> String {
        match self.field {
            FilterField::PayloadText => format!("payload ~ \"{}\"", self.value),
            _ => format!("{}: {}", self.field.label(), self.value),
        }
    }
}

// ---------------------------------------------------------------------------
// Filter state
// ---------------------------------------------------------------------------

/// Builder state for the "add filter" interaction.
#[derive(Debug, Clone, Default)]
struct AddFilterState {
    selected_field: Option<FilterField>,
    draft_value: String,
    show_popup: bool,
}

/// Tokenized filter state for Chronicle events.
///
/// Active filters are stored as `FilterToken` values. The `discover()` method
/// populates suggestions for each field from the recording's entity paths.
#[derive(Debug, Clone, Default)]
pub struct ChronicleFilter {
    tokens: Vec<FilterToken>,
    add_state: AddFilterState,

    suggestions: BTreeMap<FilterField, BTreeSet<String>>,
    /// Entity IDs grouped by entity type (for filtered dropdowns).
    entity_ids_by_type: BTreeMap<String, BTreeSet<String>>,
    has_chronicle_data: bool,
}

impl ChronicleFilter {
    /// Scan entity paths to discover available filter values.
    ///
    /// Extracts sources and event types from `{source}/{event_type}` paths,
    /// link types from `_links/` paths, and entity types/IDs from
    /// `_entities/{type}/{id}` paths logged by the bridge.
    pub fn discover(&mut self, entity_paths: impl Iterator<Item = impl AsRef<str>>) {
        let mut sources = BTreeSet::new();
        let mut event_types = BTreeSet::new();
        let mut entity_types = BTreeSet::new();
        let mut entity_ids = BTreeSet::new();
        let mut has_links = false;

        for path in entity_paths {
            let path = path.as_ref();
            let clean = path.strip_prefix('/').unwrap_or(path);

            if clean.starts_with("_links/") {
                has_links = true;
                continue;
            }

            if let Some(rest) = clean.strip_prefix("_entities/") {
                let mut parts = rest.splitn(2, '/');
                if let Some(etype) = parts.next() {
                    entity_types.insert(etype.to_owned());
                    if let Some(eid) = parts.next() {
                        entity_ids.insert(eid.to_owned());
                        self.entity_ids_by_type
                            .entry(etype.to_owned())
                            .or_default()
                            .insert(eid.to_owned());
                    }
                }
                continue;
            }

            if clean.contains("payload") || clean.is_empty() {
                continue;
            }

            let mut parts = clean.splitn(2, '/');
            if let Some(src) = parts.next() {
                if !src.is_empty() && !src.starts_with('_') {
                    sources.insert(src.to_owned());
                    if let Some(et) = parts.next() {
                        event_types.insert(et.to_owned());
                    }
                }
            }
        }

        self.has_chronicle_data = has_links || sources.len() >= 2;
        self.suggestions.insert(FilterField::Source, sources);
        self.suggestions.insert(FilterField::EventType, event_types);
        self.suggestions.insert(FilterField::EntityType, entity_types);
        self.suggestions.insert(FilterField::EntityId, entity_ids);
    }

    /// Provide entity type / entity ID suggestions (extracted from payloads
    /// elsewhere, e.g., by the bridge embedding `_entity_refs`).
    pub fn set_entity_suggestions(
        &mut self,
        entity_types: BTreeSet<String>,
        entity_ids: BTreeSet<String>,
    ) {
        self.suggestions.insert(FilterField::EntityType, entity_types);
        self.suggestions.insert(FilterField::EntityId, entity_ids);
    }

    pub fn has_chronicle_data(&self) -> bool {
        self.has_chronicle_data
    }

    pub fn is_active(&self) -> bool {
        !self.tokens.is_empty()
    }

    pub fn active_filter_count(&self) -> usize {
        self.tokens.len()
    }

    pub fn tokens(&self) -> &[FilterToken] {
        &self.tokens
    }

    pub fn clear(&mut self) {
        self.tokens.clear();
    }

    /// Check whether an entity path passes all active path-level filters
    /// (Source and EventType tokens).
    pub fn matches_path(&self, path: &str) -> bool {
        if self.tokens.is_empty() {
            return true;
        }

        let clean = path.strip_prefix('/').unwrap_or(path);
        let mut parts = clean.splitn(2, '/');
        let source = parts.next().unwrap_or("");
        let event_type = parts.next().unwrap_or("");

        for token in &self.tokens {
            match token.field {
                FilterField::Source if source != token.value => return false,
                FilterField::EventType if event_type != token.value => return false,
                _ => {}
            }
        }
        true
    }

    /// Check whether a payload JSON string passes payload-level filters
    /// (EntityType, EntityId, PayloadText tokens).
    pub fn matches_payload(&self, payload_json: &str) -> bool {
        for token in &self.tokens {
            match token.field {
                FilterField::PayloadText => {
                    if !payload_json
                        .to_ascii_lowercase()
                        .contains(&token.value.to_ascii_lowercase())
                    {
                        return false;
                    }
                }
                FilterField::EntityType | FilterField::EntityId => {
                    if !payload_has_entity_match(payload_json, token) {
                        return false;
                    }
                }
                _ => {}
            }
        }
        true
    }

    fn suggestions_for(&self, field: FilterField) -> Vec<&str> {
        if field == FilterField::EntityId {
            let active_entity_type = self
                .tokens
                .iter()
                .find(|t| t.field == FilterField::EntityType)
                .map(|t| t.value.as_str());

            if let Some(etype) = active_entity_type {
                return self
                    .entity_ids_by_type
                    .get(etype)
                    .map(|ids| ids.iter().map(String::as_str).collect())
                    .unwrap_or_default();
            }
        }

        self.suggestions
            .get(&field)
            .map(|s| s.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // UI
    // -----------------------------------------------------------------------

    /// Render the tokenized filter bar. Returns `true` if filters changed.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        if !self.has_chronicle_data {
            return false;
        }

        let mut changed = false;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().item_spacing.y = 2.0;

            let mut to_remove = None;
            for (i, token) in self.tokens.iter().enumerate() {
                let chip = egui::Button::new(
                    egui::RichText::new(format!("{}  ✕", token.chip_label()))
                        .small()
                        .color(egui::Color32::WHITE),
                )
                .fill(field_color(token.field))
                .rounding(egui::Rounding::same(10));

                if ui.add(chip).on_hover_text("Click to remove").clicked() {
                    to_remove = Some(i);
                }
            }
            if let Some(i) = to_remove {
                self.tokens.remove(i);
                changed = true;
            }

            let btn_text = if self.tokens.is_empty() { "+ Filter" } else { "+" };
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(btn_text).small())
                        .rounding(egui::Rounding::same(10))
                        .stroke(egui::Stroke::new(
                            1.0,
                            ui.visuals().widgets.inactive.fg_stroke.color,
                        ))
                        .fill(egui::Color32::TRANSPARENT),
                )
                .clicked()
            {
                self.add_state.show_popup = !self.add_state.show_popup;
                self.add_state.selected_field = None;
                self.add_state.draft_value.clear();
            }

            if self.tokens.len() > 1
                && ui
                    .add(
                        egui::Button::new(egui::RichText::new("Clear all").small().weak())
                            .frame(false),
                    )
                    .clicked()
            {
                self.clear();
                changed = true;
            }
        });

        if self.add_state.show_popup {
            self.add_filter_inline_ui(ui, &mut changed);
        }

        changed
    }

    fn add_filter_inline_ui(&mut self, ui: &mut egui::Ui, changed: &mut bool) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            if self.add_state.selected_field.is_none() {
                for &field in FilterField::ALL {
                    let btn = egui::Button::new(egui::RichText::new(field.label()).small())
                        .fill(field_color(field).gamma_multiply(0.15))
                        .stroke(egui::Stroke::new(1.0, field_color(field)));
                    if ui.add(btn).clicked() {
                        self.add_state.selected_field = Some(field);
                        self.add_state.draft_value.clear();
                    }
                }

                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("Cancel").small().weak())
                            .frame(false),
                    )
                    .clicked()
                {
                    self.add_state.show_popup = false;
                }
            } else {
                let field = self.add_state.selected_field.unwrap();

                ui.label(
                    egui::RichText::new(format!("{}:", field.label()))
                        .small()
                        .color(field_color(field)),
                );

                let suggestions: Vec<String> = self
                    .suggestions
                    .get(&field)
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default();

                if field.has_suggestions() && !suggestions.is_empty() {
                    let mut selected = self.add_state.draft_value.clone();
                    egui::ComboBox::from_id_salt("chronicle_filter_value")
                        .selected_text(if selected.is_empty() {
                            "Select…"
                        } else {
                            &selected
                        })
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for s in &suggestions {
                                ui.selectable_value(&mut selected, s.clone(), s.as_str());
                            }
                        });

                    if !selected.is_empty() && selected != self.add_state.draft_value {
                        self.tokens.push(FilterToken {
                            field,
                            value: selected,
                        });
                        self.add_state = AddFilterState::default();
                        *changed = true;
                    }
                } else {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.add_state.draft_value)
                            .hint_text(if field == FilterField::PayloadText {
                                "Search text…"
                            } else {
                                "Value…"
                            })
                            .desired_width(150.0),
                    );

                    let submitted =
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                    if submitted && !self.add_state.draft_value.is_empty() {
                        self.tokens.push(FilterToken {
                            field,
                            value: std::mem::take(&mut self.add_state.draft_value),
                        });
                        self.add_state = AddFilterState::default();
                        *changed = true;
                    }
                }

                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("← Back").small().weak())
                            .frame(false),
                    )
                    .clicked()
                {
                    self.add_state.selected_field = None;
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn payload_has_entity_match(json_str: &str, token: &FilterToken) -> bool {
    let Some(start) = json_str.find("\"_entity_refs\"") else {
        return false;
    };
    let remainder = &json_str[start..];

    match token.field {
        FilterField::EntityType => {
            remainder.contains(&format!("\"type\":\"{}\"", token.value))
                || remainder.contains(&format!("\"type\": \"{}\"", token.value))
        }
        FilterField::EntityId => {
            remainder.contains(&format!("\"id\":\"{}\"", token.value))
                || remainder.contains(&format!("\"id\": \"{}\"", token.value))
        }
        _ => true,
    }
}

fn field_color(field: FilterField) -> egui::Color32 {
    match field {
        FilterField::Source => egui::Color32::from_rgb(99, 102, 241),
        FilterField::EventType => egui::Color32::from_rgb(16, 185, 129),
        FilterField::EntityType => egui::Color32::from_rgb(245, 158, 11),
        FilterField::EntityId => egui::Color32::from_rgb(239, 68, 68),
        FilterField::PayloadText => egui::Color32::from_rgb(139, 92, 246),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_paths() -> Vec<&'static str> {
        vec![
            "/stripe/charge.created",
            "/stripe/payment_intent.succeeded",
            "/stripe/payment_intent.failed",
            "/support/ticket.created",
            "/support/ticket.closed",
            "/billing/invoice.created",
            "/billing/invoice.overdue",
            "/marketing/email.clicked",
            "/product/page.viewed",
            "/stripe/charge.created/payload",
            "/_links/stripe/payment_intent.failed/123/to/support/ticket.created/456/caused_by",
            "/_links/support/ticket.escalated/789/to/stripe/subscription.cancelled/101/led_to",
            "/_entities/customer/cust_001",
            "/_entities/customer/cust_002",
            "/_entities/customer/cust_003",
            "/_entities/account/acc_1",
            "/_entities/ticket/tkt_42",
        ]
    }

    #[test]
    fn discover_finds_sources_and_event_types() {
        let mut f = ChronicleFilter::default();
        f.discover(sample_paths().into_iter());

        let sources = f.suggestions.get(&FilterField::Source).unwrap();
        assert_eq!(sources.len(), 5);
        assert!(sources.contains("stripe"));
        assert!(sources.contains("support"));
        assert!(sources.contains("billing"));
        assert!(sources.contains("marketing"));
        assert!(sources.contains("product"));

        let event_types = f.suggestions.get(&FilterField::EventType).unwrap();
        assert!(event_types.contains("charge.created"));
        assert!(event_types.contains("ticket.created"));
        assert!(event_types.contains("invoice.overdue"));
    }

    #[test]
    fn discover_detects_chronicle_data() {
        let mut f = ChronicleFilter::default();
        f.discover(sample_paths().into_iter());
        assert!(f.has_chronicle_data());
    }

    #[test]
    fn discover_skips_payload_and_internal_paths() {
        let mut f = ChronicleFilter::default();
        f.discover(vec!["/stripe/charge.created/payload", "", "/_internal"].into_iter());
        assert!(f.suggestions.get(&FilterField::Source).unwrap().is_empty());
    }

    #[test]
    fn matches_path_no_tokens() {
        let f = ChronicleFilter::default();
        assert!(f.matches_path("/stripe/charge.created"));
        assert!(f.matches_path("/support/ticket.created"));
    }

    #[test]
    fn matches_path_source_token() {
        let mut f = ChronicleFilter::default();
        f.tokens.push(FilterToken {
            field: FilterField::Source,
            value: "stripe".to_owned(),
        });

        assert!(f.matches_path("/stripe/charge.created"));
        assert!(f.matches_path("stripe/payment_intent.failed"));
        assert!(!f.matches_path("/support/ticket.created"));
        assert!(!f.matches_path("/billing/invoice.overdue"));
    }

    #[test]
    fn matches_path_event_type_token() {
        let mut f = ChronicleFilter::default();
        f.tokens.push(FilterToken {
            field: FilterField::EventType,
            value: "charge.created".to_owned(),
        });

        assert!(f.matches_path("/stripe/charge.created"));
        assert!(!f.matches_path("/stripe/payment_intent.failed"));
    }

    #[test]
    fn matches_path_combined_tokens() {
        let mut f = ChronicleFilter::default();
        f.tokens.push(FilterToken {
            field: FilterField::Source,
            value: "stripe".to_owned(),
        });
        f.tokens.push(FilterToken {
            field: FilterField::EventType,
            value: "charge.created".to_owned(),
        });

        assert!(f.matches_path("/stripe/charge.created"));
        assert!(!f.matches_path("/stripe/payment_intent.failed"));
        assert!(!f.matches_path("/billing/charge.created"));
    }

    #[test]
    fn matches_payload_text() {
        let mut f = ChronicleFilter::default();
        f.tokens.push(FilterToken {
            field: FilterField::PayloadText,
            value: "card_declined".to_owned(),
        });

        assert!(f.matches_payload(r#"{"failure_code":"card_declined"}"#));
        assert!(f.matches_payload(r#"{"msg":"CARD_DECLINED error"}"#));
        assert!(!f.matches_payload(r#"{"failure_code":"insufficient_funds"}"#));
    }

    #[test]
    fn matches_payload_entity_type() {
        let mut f = ChronicleFilter::default();
        f.tokens.push(FilterToken {
            field: FilterField::EntityType,
            value: "customer".to_owned(),
        });

        assert!(f.matches_payload(
            r#"{"_entity_refs":[{"type":"customer","id":"cust_001"}]}"#
        ));
        assert!(f.matches_payload(
            r#"{"_entity_refs":[{"type": "customer", "id": "cust_002"}]}"#
        ));
        assert!(!f.matches_payload(
            r#"{"_entity_refs":[{"type":"account","id":"acc_1"}]}"#
        ));
        assert!(!f.matches_payload(r#"{"amount":100}"#));
    }

    #[test]
    fn matches_payload_entity_id() {
        let mut f = ChronicleFilter::default();
        f.tokens.push(FilterToken {
            field: FilterField::EntityId,
            value: "cust_001".to_owned(),
        });

        assert!(f.matches_payload(
            r#"{"_entity_refs":[{"type":"customer","id":"cust_001"}]}"#
        ));
        assert!(!f.matches_payload(
            r#"{"_entity_refs":[{"type":"customer","id":"cust_002"}]}"#
        ));
    }

    #[test]
    fn token_chip_labels() {
        assert_eq!(
            FilterToken { field: FilterField::Source, value: "stripe".into() }.chip_label(),
            "Source: stripe"
        );
        assert_eq!(
            FilterToken { field: FilterField::PayloadText, value: "error".into() }.chip_label(),
            "payload ~ \"error\""
        );
    }

    #[test]
    fn clear_removes_all_tokens() {
        let mut f = ChronicleFilter::default();
        f.tokens.push(FilterToken { field: FilterField::Source, value: "a".into() });
        f.tokens.push(FilterToken { field: FilterField::EventType, value: "b".into() });
        assert!(f.is_active());
        assert_eq!(f.active_filter_count(), 2);

        f.clear();
        assert!(!f.is_active());
        assert_eq!(f.active_filter_count(), 0);
    }

    #[test]
    fn discover_finds_entity_types_and_ids() {
        let mut f = ChronicleFilter::default();
        f.discover(sample_paths().into_iter());

        let entity_types = f.suggestions.get(&FilterField::EntityType).unwrap();
        assert_eq!(entity_types.len(), 3);
        assert!(entity_types.contains("customer"));
        assert!(entity_types.contains("account"));
        assert!(entity_types.contains("ticket"));

        let entity_ids = f.suggestions.get(&FilterField::EntityId).unwrap();
        assert_eq!(entity_ids.len(), 5);
        assert!(entity_ids.contains("cust_001"));
        assert!(entity_ids.contains("acc_1"));
        assert!(entity_ids.contains("tkt_42"));
    }

    #[test]
    fn entity_ids_grouped_by_type() {
        let mut f = ChronicleFilter::default();
        f.discover(sample_paths().into_iter());

        let customer_ids = f.entity_ids_by_type.get("customer").unwrap();
        assert_eq!(customer_ids.len(), 3);
        assert!(customer_ids.contains("cust_001"));
        assert!(customer_ids.contains("cust_002"));
        assert!(customer_ids.contains("cust_003"));

        let account_ids = f.entity_ids_by_type.get("account").unwrap();
        assert_eq!(account_ids.len(), 1);
        assert!(account_ids.contains("acc_1"));
    }

    #[test]
    fn entity_id_suggestions_filtered_by_active_type() {
        let mut f = ChronicleFilter::default();
        f.discover(sample_paths().into_iter());

        let all_ids = f.suggestions_for(FilterField::EntityId);
        assert_eq!(all_ids.len(), 5);

        f.tokens.push(FilterToken {
            field: FilterField::EntityType,
            value: "customer".to_owned(),
        });
        let filtered_ids = f.suggestions_for(FilterField::EntityId);
        assert_eq!(filtered_ids.len(), 3);
        assert!(filtered_ids.contains(&"cust_001"));
        assert!(!filtered_ids.contains(&"acc_1"));
    }

    #[test]
    fn field_colors_are_distinct() {
        let colors: Vec<_> = FilterField::ALL.iter().map(|f| field_color(*f)).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }
}
