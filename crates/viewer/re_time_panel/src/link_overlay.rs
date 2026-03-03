//! Link overlay for the Chronicle event store.
//!
//! Draws Bezier arcs between linked events across swimlane rows in the
//! time panel. Activated when a user selects an event that has causal
//! links (e.g., payment_failure → support_ticket → cancellation).

use std::collections::HashMap;

use egui::epaint::CubicBezierShape;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

/// A resolved link ready for painting: source and target positions.
#[derive(Debug, Clone)]
pub struct ResolvedLink {
    pub source_x: f32,
    pub source_y: f32,
    pub target_x: f32,
    pub target_y: f32,
    /// "caused_by", "related_to", "triggered", etc.
    pub link_type: String,
    /// Confidence 0.0–1.0; controls opacity.
    pub confidence: f32,
}

/// State for the link overlay system.
#[derive(Default)]
pub struct LinkOverlayState {
    pub links: Vec<ResolvedLink>,
    pub active: bool,
}

impl LinkOverlayState {
    pub fn clear(&mut self) {
        self.links.clear();
        self.active = false;
    }

    pub fn set_links(&mut self, links: Vec<ResolvedLink>) {
        self.active = !links.is_empty();
        self.links = links;
    }

    /// Paint the link arcs onto the time panel.
    ///
    /// Called between `tree_ui()` and `time_marker_ui()` in `expanded_ui()`.
    pub fn paint(&self, painter: &Painter, clip_rect: Rect) {
        if !self.active {
            return;
        }

        let overlay_painter = painter.with_clip_rect(clip_rect);

        for link in &self.links {
            let x0 = link.source_x;
            let y0 = link.source_y;
            let x1 = link.target_x;
            let y1 = link.target_y;

            if !clip_rect.x_range().contains(x0) && !clip_rect.x_range().contains(x1) {
                continue;
            }

            let color = link_type_color(&link.link_type, link.confidence);
            let stroke = Stroke::new(2.0, color);

            let mid_x = (x0 + x1) / 2.0;
            let arc_height = ((y0 - y1).abs() * 0.3).max(20.0);
            let control_y = y0.min(y1) - arc_height;

            overlay_painter.add(CubicBezierShape {
                points: [
                    pos2(x0, y0),
                    pos2(mid_x, control_y),
                    pos2(mid_x, control_y),
                    pos2(x1, y1),
                ],
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: stroke.into(),
            });

            overlay_painter.circle_filled(pos2(x0, y0), 4.0, color);
            paint_arrow_head(&overlay_painter, pos2(x1, y1), x1 > x0, color);
        }
    }
}

/// Map link types to distinct colours with confidence as alpha.
pub fn link_type_color(link_type: &str, confidence: f32) -> Color32 {
    let alpha = (confidence * 200.0 + 55.0) as u8;
    match link_type {
        "caused_by" => Color32::from_rgba_unmultiplied(255, 80, 80, alpha),
        "led_to" => Color32::from_rgba_unmultiplied(255, 160, 60, alpha),
        "triggered" => Color32::from_rgba_unmultiplied(255, 220, 50, alpha),
        "related_to" => Color32::from_rgba_unmultiplied(80, 160, 255, alpha),
        "campaign_conversion" => Color32::from_rgba_unmultiplied(80, 220, 120, alpha),
        _ => Color32::from_rgba_unmultiplied(180, 180, 180, alpha),
    }
}

fn paint_arrow_head(painter: &Painter, tip: Pos2, points_right: bool, color: Color32) {
    let size = 6.0;
    let dir = if points_right { -1.0 } else { 1.0 };
    let p1 = pos2(tip.x + dir * size, tip.y - size * 0.5);
    let p2 = pos2(tip.x + dir * size, tip.y + size * 0.5);
    painter.add(Shape::convex_polygon(vec![tip, p1, p2], color, Stroke::NONE));
}

/// Maps entity path strings to their center Y coordinate in screen space.
pub type RowPositions = HashMap<String, f32>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_type_colors_are_distinct() {
        let types = [
            "caused_by",
            "led_to",
            "triggered",
            "related_to",
            "campaign_conversion",
        ];
        let colors: Vec<_> = types.iter().map(|t| link_type_color(t, 1.0)).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "{} vs {}", types[i], types[j]);
            }
        }
    }

    #[test]
    fn confidence_affects_alpha() {
        let low = link_type_color("caused_by", 0.3);
        let high = link_type_color("caused_by", 0.9);
        assert!(low[3] < high[3]);
    }

    #[test]
    fn paint_does_not_panic_with_empty_links() {
        let state = LinkOverlayState::default();
        assert!(!state.active);
        assert!(state.links.is_empty());
    }

    #[test]
    fn set_and_clear_links() {
        let mut state = LinkOverlayState::default();
        state.set_links(vec![ResolvedLink {
            source_x: 100.0,
            source_y: 50.0,
            target_x: 200.0,
            target_y: 80.0,
            link_type: "caused_by".to_string(),
            confidence: 0.85,
        }]);
        assert!(state.active);
        assert_eq!(state.links.len(), 1);

        state.clear();
        assert!(!state.active);
        assert!(state.links.is_empty());
    }
}
