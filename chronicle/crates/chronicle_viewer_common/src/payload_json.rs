//! JSON payload viewer widget with syntax-highlighted pretty printing.

use egui::Ui;

/// Displays a JSON payload with syntax highlighting and collapsible sections.
pub struct PayloadJsonWidget<'a> {
    payload: Option<&'a serde_json::Value>,
}

impl<'a> PayloadJsonWidget<'a> {
    pub fn new(payload: Option<&'a serde_json::Value>) -> Self {
        Self { payload }
    }

    pub fn ui(&self, ui: &mut Ui) {
        match self.payload {
            None => {
                ui.label(egui::RichText::new("No payload").weak().italics());
            }
            Some(value) => {
                let json_str = serde_json::to_string_pretty(value).unwrap_or_default();

                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        let mut layouter =
                            |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap_width: f32| {
                                let job = highlight_json(ui, text.as_str());
                                ui.ctx().fonts_mut(|f| f.layout_job(job))
                            };

                        let mut text = json_str.clone();
                        ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .interactive(false)
                                .layouter(&mut layouter),
                        );
                    });
            }
        }
    }
}

/// Basic JSON syntax highlighting via egui layout jobs.
fn highlight_json(ui: &egui::Ui, text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::monospace(12.0);

    let key_color = egui::Color32::from_rgb(156, 220, 254);
    let string_color = egui::Color32::from_rgb(206, 145, 120);
    let number_color = egui::Color32::from_rgb(181, 206, 168);
    let bool_color = egui::Color32::from_rgb(86, 156, 214);
    let null_color = egui::Color32::from_rgb(86, 156, 214);
    let punct_color = ui.visuals().text_color();

    let mut chars = text.chars().peekable();
    let mut current = String::new();
    let mut in_string = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if !in_string => {
                flush(&mut job, &current, punct_color, &font);
                current.clear();
                in_string = true;
                current.push('"');
            }
            '"' if in_string => {
                current.push('"');
                let rest: String = chars.clone().collect();
                let trimmed = rest.trim_start();
                let color = if trimmed.starts_with(':') {
                    key_color
                } else {
                    string_color
                };
                flush(&mut job, &current, color, &font);
                current.clear();
                in_string = false;
            }
            _ if in_string => {
                current.push(ch);
            }
            't' | 'f' if !in_string => {
                flush(&mut job, &current, punct_color, &font);
                current.clear();
                current.push(ch);
                while chars.peek().is_some_and(|c| c.is_alphabetic()) {
                    current.push(chars.next().unwrap());
                }
                flush(&mut job, &current, bool_color, &font);
                current.clear();
            }
            'n' if !in_string => {
                flush(&mut job, &current, punct_color, &font);
                current.clear();
                current.push(ch);
                while chars.peek().is_some_and(|c| c.is_alphabetic()) {
                    current.push(chars.next().unwrap());
                }
                flush(&mut job, &current, null_color, &font);
                current.clear();
            }
            '0'..='9' | '-' | '.' if !in_string && current.trim().is_empty() => {
                flush(&mut job, &current, punct_color, &font);
                current.clear();
                current.push(ch);
                while chars.peek().is_some_and(|c| {
                    c.is_ascii_digit()
                        || *c == '.'
                        || *c == 'e'
                        || *c == 'E'
                        || *c == '+'
                        || *c == '-'
                }) {
                    current.push(chars.next().unwrap());
                }
                flush(&mut job, &current, number_color, &font);
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }
    flush(&mut job, &current, punct_color, &font);

    job
}

fn flush(job: &mut egui::text::LayoutJob, text: &str, color: egui::Color32, font: &egui::FontId) {
    if !text.is_empty() {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_widget_handles_none() {
        let _widget = PayloadJsonWidget::new(None);
    }

    #[test]
    fn payload_widget_handles_complex_json() {
        let val = serde_json::json!({
            "amount": 4999,
            "currency": "usd",
            "nested": {"key": "value"},
            "array": [1, 2, 3],
            "null_field": null,
            "bool_field": true
        });
        let _widget = PayloadJsonWidget::new(Some(&val));
    }
}
