//! Editor de código com realce de sintaxe e popup de autocomplete.

use crate::completion::{self, CARET, EditorKind, Suggestion};
use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Color32, FontId, Id, Key, Modifiers, TextFormat, TextStyle, Ui};

#[derive(Clone, Copy)]
pub struct Palette {
    pub key: Color32,
    pub string: Color32,
    pub number: Color32,
    pub operator: Color32,
    pub field_ref: Color32,
    pub func: Color32,
    pub keyword: Color32,
    pub punct: Color32,
    pub comment: Color32,
    pub regex: Color32,
}

impl Palette {
    pub fn for_ui(ui: &Ui) -> Self {
        if ui.visuals().dark_mode {
            Self {
                key: Color32::from_rgb(0x9C, 0xDC, 0xFE),
                string: Color32::from_rgb(0xCE, 0x91, 0x78),
                number: Color32::from_rgb(0xB5, 0xCE, 0xA8),
                operator: Color32::from_rgb(0xC5, 0x86, 0xC0),
                field_ref: Color32::from_rgb(0x4E, 0xC9, 0xB0),
                func: Color32::from_rgb(0xDC, 0xDC, 0xAA),
                keyword: Color32::from_rgb(0x56, 0x9C, 0xD6),
                punct: Color32::from_rgb(0xC8, 0xC8, 0xC8),
                comment: Color32::from_rgb(0x6A, 0x99, 0x55),
                regex: Color32::from_rgb(0xD1, 0x69, 0x69),
            }
        } else {
            Self {
                key: Color32::from_rgb(0x04, 0x51, 0xA5),
                string: Color32::from_rgb(0xA3, 0x15, 0x15),
                number: Color32::from_rgb(0x09, 0x86, 0x58),
                operator: Color32::from_rgb(0xAF, 0x00, 0xDB),
                field_ref: Color32::from_rgb(0x26, 0x7F, 0x99),
                func: Color32::from_rgb(0x79, 0x5E, 0x26),
                keyword: Color32::from_rgb(0x00, 0x00, 0xFF),
                punct: Color32::from_rgb(0x33, 0x33, 0x33),
                comment: Color32::from_rgb(0x00, 0x80, 0x00),
                regex: Color32::from_rgb(0x81, 0x1F, 0x3F),
            }
        }
    }
}

/// Gera o `LayoutJob` com cores para o texto em sintaxe shell.
pub fn highlight(text: &str, font: FontId, pal: &Palette) -> LayoutJob {
    let mut job = LayoutJob::default();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let fmt = |color: Color32| TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };
    let next_sig = |from: usize| chars[from..].iter().find(|c| !c.is_whitespace()).copied();
    // Último caractere significativo: decide se '/' começa uma regex.
    let mut prev_sig: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        let color;
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            color = pal.comment;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            color = pal.comment;
        } else if c == '"' || c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != c && chars[i] != '\n' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(chars.len());
            let is_key = next_sig(i) == Some(':');
            color = if is_key {
                pal.key
            } else if chars.get(start + 1) == Some(&'$') {
                pal.field_ref
            } else {
                pal.string
            };
        } else if c == '/' && matches!(prev_sig, None | Some(':') | Some(',') | Some('[') | Some('(')) {
            i += 1;
            while i < chars.len() && chars[i] != '/' && chars[i] != '\n' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(chars.len());
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            color = pal.regex;
        } else if c.is_ascii_digit() || (c == '-' && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit())) {
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            color = if next_sig(i) == Some(':') { pal.key } else { pal.number };
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$' || chars[i] == '.') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let after = next_sig(i);
            color = if word.starts_with('$') {
                pal.operator
            } else if after == Some(':') {
                pal.key
            } else if after == Some('(') || word == "new" {
                pal.func
            } else if matches!(word.as_str(), "true" | "false" | "null" | "undefined" | "NaN" | "Infinity") {
                pal.keyword
            } else {
                pal.punct
            };
        } else {
            i += 1;
            color = pal.punct;
        }
        let piece: String = chars[start..i].iter().collect();
        if let Some(sig) = piece.chars().rev().find(|c| !c.is_whitespace()) {
            prev_sig = Some(if piece.len() > 1 { 'a' } else { sig });
        }
        job.append(&piece, 0.0, fmt(color));
    }
    if chars.is_empty() {
        job.append("", 0.0, fmt(pal.punct));
    }
    job
}

#[derive(Clone, Default)]
struct AcState {
    open: bool,
    items: Vec<Suggestion>,
    selected: usize,
    start: usize,
    end: usize,
    scroll_to_selected: bool,
    popup_rect: Option<egui::Rect>,
}

pub struct EditorOutput {
    pub changed: bool,
    pub submit: bool,
}

pub struct CodeEditor<'a> {
    id: Id,
    text: &'a mut String,
    kind: EditorKind,
    fields: &'a [String],
    rows: usize,
    submit_on_enter: bool,
    hint: &'a str,
    width: f32,
}

impl<'a> CodeEditor<'a> {
    pub fn new(id: Id, text: &'a mut String, kind: EditorKind, fields: &'a [String]) -> Self {
        Self {
            id,
            text,
            kind,
            fields,
            rows: 1,
            submit_on_enter: false,
            hint: "",
            width: f32::INFINITY,
        }
    }

    /// Enter executa (Shift+Enter quebra linha). Sem isso, Ctrl+Enter executa.
    pub fn submit_on_enter(mut self, v: bool) -> Self {
        self.submit_on_enter = v;
        self
    }

    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows;
        self
    }

    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn show(self, ui: &mut Ui) -> EditorOutput {
        let CodeEditor {
            id,
            text,
            kind,
            fields,
            rows,
            submit_on_enter,
            hint,
            width,
        } = self;
        let ac_id = id.with("autocomplete");
        let mut st: AcState = ui.data(|d| d.get_temp(ac_id)).unwrap_or_default();
        let had_focus = ui.memory(|m| m.has_focus(id));
        let mut accept: Option<usize> = None;
        let mut submit = false;
        let mut forced = false;

        if had_focus {
            ui.input_mut(|i| {
                if st.open && !st.items.is_empty() {
                    let n = st.items.len();
                    if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                        st.selected = (st.selected + 1) % n;
                        st.scroll_to_selected = true;
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                        st.selected = (st.selected + n - 1) % n;
                        st.scroll_to_selected = true;
                    }
                    if i.consume_key(Modifiers::NONE, Key::PageDown) {
                        st.selected = (st.selected + 8).min(n - 1);
                        st.scroll_to_selected = true;
                    }
                    if i.consume_key(Modifiers::NONE, Key::PageUp) {
                        st.selected = st.selected.saturating_sub(8);
                        st.scroll_to_selected = true;
                    }
                    if i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::Tab) {
                        accept = Some(st.selected);
                    }
                    if i.consume_key(Modifiers::NONE, Key::Escape) {
                        st.open = false;
                    }
                }
                if i.consume_key(Modifiers::COMMAND, Key::Enter) {
                    submit = true;
                }
                if submit_on_enter && accept.is_none() && i.consume_key(Modifiers::NONE, Key::Enter) {
                    submit = true;
                }
                if i.consume_key(Modifiers::CTRL, Key::Space) {
                    forced = true;
                }
            });
        }

        let pal = Palette::for_ui(ui);
        let font = TextStyle::Monospace.resolve(ui.style());
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = highlight(buf.as_str(), font.clone(), &pal);
            job.wrap.max_width = wrap_width;
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };
        let mut te = egui::TextEdit::multiline(text)
            .id(id)
            .font(TextStyle::Monospace)
            .desired_rows(rows)
            .desired_width(width)
            .hint_text(hint)
            .layouter(&mut layouter);
        if submit_on_enter {
            te = te.return_key(egui::KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter));
        } else {
            te = te.lock_focus(true);
        }
        let out = te.show(ui);
        let response = &out.response.response;
        let mut changed = response.changed();
        let focused = response.has_focus();
        let cursor = out.cursor_range.map(|r| r.primary.index.0);

        if accept.is_none() {
            if let (true, Some(c)) = (changed || forced, cursor) {
                let chars: Vec<char> = text.chars().collect();
                let start = completion::prefix_at(&chars, c);
                let items = completion::suggest(kind, &chars, start, c, fields, forced);
                st.open = !items.is_empty() && (forced || c > start);
                st.items = items;
                st.selected = 0;
                st.start = start;
                st.end = c;
                st.scroll_to_selected = true;
            } else if st.open && cursor.is_some() && cursor != Some(st.end) {
                st.open = false;
            }
        }

        let hovering_popup = st
            .popup_rect
            .zip(ui.ctx().pointer_hover_pos())
            .is_some_and(|(r, p)| r.contains(p));
        if !focused && !had_focus && !hovering_popup {
            st.open = false;
        }

        if st.open && !st.items.is_empty() {
            let caret = out
                .galley
                .pos_from_cursor(CCursor::new(st.start.min(text.chars().count())))
                .translate(out.galley_pos.to_vec2());
            let area = egui::Area::new(id.with("ac_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(caret.left_bottom() + egui::vec2(0.0, 2.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(320.0);
                        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                            for (i, item) in st.items.iter().enumerate() {
                                let selected = i == st.selected;
                                let resp = ui
                                    .horizontal(|ui| {
                                        let r = ui.selectable_label(
                                            selected,
                                            egui::RichText::new(&item.label).monospace().color(kind_color(item.kind, &pal)),
                                        );
                                        ui.label(egui::RichText::new(item.kind.label()).small().weak());
                                        if !item.detail.is_empty() {
                                            ui.label(egui::RichText::new(&item.detail).small().weak());
                                        }
                                        r
                                    })
                                    .inner;
                                if selected && st.scroll_to_selected {
                                    resp.scroll_to_me(None);
                                }
                                if resp.clicked() {
                                    accept = Some(i);
                                }
                            }
                        });
                        ui.label(egui::RichText::new("⬆⬇ navegar · Enter/Tab aceitar · Esc fechar").small().weak());
                    });
                });
            st.popup_rect = Some(area.response.rect);
            st.scroll_to_selected = false;
        } else {
            st.popup_rect = None;
        }

        if let Some(i) = accept {
            if let Some(item) = st.items.get(i).cloned() {
                let chars: Vec<char> = text.chars().collect();
                let start = st.start.min(chars.len());
                let end = st.end.clamp(start, chars.len());
                let mut insert = item.insert.clone();
                // Dentro de uma string só o nome é inserido.
                let before: String = chars[..start].iter().collect();
                if before.matches('"').count() % 2 == 1 {
                    insert = item.label.clone();
                }
                let caret_pos = insert.chars().position(|c| c == CARET);
                let insert_clean: String = insert.chars().filter(|c| *c != CARET).collect();
                let caret_pos = caret_pos.unwrap_or(insert_clean.chars().count());
                let new_text: String = chars[..start]
                    .iter()
                    .copied()
                    .chain(insert_clean.chars())
                    .chain(chars[end..].iter().copied())
                    .collect();
                *text = new_text;
                let new_cursor = start + caret_pos;
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(new_cursor))));
                    state.store(ui.ctx(), id);
                }
                ui.memory_mut(|m| m.request_focus(id));
                changed = true;
            }
            st.open = false;
            st.popup_rect = None;
        }

        ui.data_mut(|d| d.insert_temp(ac_id, st));
        EditorOutput { changed, submit }
    }
}

fn kind_color(kind: completion::ItemKind, pal: &Palette) -> Color32 {
    use completion::ItemKind::*;
    match kind {
        Field => pal.key,
        FieldRef => pal.field_ref,
        QueryOp | UpdateOp | Stage | Expr => pal.operator,
        Function => pal.func,
        Value => pal.number,
    }
}

/// Exibe texto com realce (somente leitura, selecionável).
pub fn code_view(ui: &mut Ui, text: &str) -> egui::Response {
    let pal = Palette::for_ui(ui);
    let font = TextStyle::Monospace.resolve(ui.style());
    let mut job = highlight(text, font, &pal);
    job.wrap.max_width = f32::INFINITY;
    ui.add(egui::Label::new(job).selectable(true).extend())
}
