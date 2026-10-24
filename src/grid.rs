//! Visualização de documentos em tabela (colunas redimensionáveis, ordenação
//! pelo cabeçalho, rolagem virtual) e em lista JSON.

use crate::editor;
use crate::shell;
use bson::{Bson, Document};
use egui::{Align, Color32, Id, Layout, RichText, Sense, TextStyle, Ui};
use egui_extras::{Column, TableBuilder};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    Missing,
    Null,
    Bool,
    Number,
    String,
    ObjectId,
    Date,
    Object,
    Array,
    Other,
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub text: String,
    pub kind: CellKind,
}

/// Dados pré-formatados de uma página de resultados.
pub struct GridData {
    pub docs: Vec<Document>,
    pub columns: Vec<String>,
    pub cells: Vec<Vec<Cell>>,
    pub json: Vec<String>,
    pub json_lines: Vec<usize>,
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut out: String = s
        .chars()
        .take(max)
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

pub fn cell_for(v: Option<&Bson>) -> Cell {
    let Some(v) = v else {
        return Cell {
            text: String::new(),
            kind: CellKind::Missing,
        };
    };
    let (text, kind) = match v {
        Bson::String(s) => (truncate_chars(s, 300), CellKind::String),
        Bson::Int32(n) => (n.to_string(), CellKind::Number),
        Bson::Int64(n) => (n.to_string(), CellKind::Number),
        Bson::Double(n) => (n.to_string(), CellKind::Number),
        Bson::Decimal128(d) => (d.to_string(), CellKind::Number),
        Bson::Boolean(b) => (b.to_string(), CellKind::Bool),
        Bson::Null => ("null".into(), CellKind::Null),
        Bson::Undefined => ("undefined".into(), CellKind::Null),
        Bson::ObjectId(o) => (o.to_hex(), CellKind::ObjectId),
        Bson::DateTime(d) => (shell::format_date(d), CellKind::Date),
        Bson::Document(d) => (
            truncate_chars(&shell::doc_to_shell_compact(d), 300),
            CellKind::Object,
        ),
        Bson::Array(a) => (
            format!("[{}] {}", a.len(), truncate_chars(&shell::to_shell_compact(v), 300)),
            CellKind::Array,
        ),
        other => (truncate_chars(&shell::to_shell_compact(other), 300), CellKind::Other),
    };
    Cell { text, kind }
}

impl GridData {
    pub fn build(docs: Vec<Document>) -> Self {
        let mut columns: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for d in &docs {
            for k in d.keys() {
                if seen.insert(k.clone()) {
                    columns.push(k.clone());
                }
            }
        }
        if let Some(pos) = columns.iter().position(|c| c == "_id") {
            let id = columns.remove(pos);
            columns.insert(0, id);
        }
        let cells = docs
            .iter()
            .map(|d| columns.iter().map(|c| cell_for(d.get(c))).collect())
            .collect();
        let json: Vec<String> = docs.iter().map(shell::doc_to_shell_pretty).collect();
        let json_lines = json.iter().map(|s| s.lines().count().max(1)).collect();
        Self {
            docs,
            columns,
            cells,
            json,
            json_lines,
        }
    }
}

#[derive(Default)]
pub struct GridState {
    pub selected: Option<usize>,
    pub widths: HashMap<String, f32>,
    pub hidden: HashSet<String>,
    pub scroll_to: Option<usize>,
    /// Muda para descartar as larguras guardadas pela tabela.
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridAction {
    /// Ordena pelo campo (1, -1 ou 0 para remover). `append` = manter as outras chaves.
    Sort { field: String, dir: i32, append: bool },
    OpenDocument(usize),
    EditCell(usize, String),
    CopyValue(usize, String),
    CopyDocument(usize),
    CloneDocument(usize),
    DeleteDocument(usize),
    FilterBy(usize, String),
}

pub fn cell_color(kind: CellKind, ui: &Ui) -> Color32 {
    let dark = ui.visuals().dark_mode;
    let pick = |d: (u8, u8, u8), l: (u8, u8, u8)| {
        let (r, g, b) = if dark { d } else { l };
        Color32::from_rgb(r, g, b)
    };
    match kind {
        CellKind::String => ui.visuals().text_color(),
        CellKind::Number => pick((0x8F, 0xC9, 0xFF), (0x00, 0x5C, 0xC5)),
        CellKind::Bool => pick((0xFF, 0xB8, 0x6C), (0xB3, 0x5C, 0x00)),
        CellKind::Null | CellKind::Missing => ui.visuals().weak_text_color(),
        CellKind::ObjectId => pick((0xD2, 0xA8, 0xFF), (0x82, 0x3A, 0xC0)),
        CellKind::Date => pick((0x7E, 0xE7, 0x87), (0x1A, 0x7F, 0x37)),
        CellKind::Object | CellKind::Array => pick((0xA0, 0xA8, 0xB4), (0x55, 0x5E, 0x6A)),
        CellKind::Other => pick((0xF2, 0xCC, 0x60), (0x8A, 0x6D, 0x00)),
    }
}

fn estimate_width(data: &GridData, col: usize, char_w: f32) -> f32 {
    let header = data.columns[col].chars().count() + 3;
    let max_cell = data
        .cells
        .iter()
        .take(100)
        .map(|r| r[col].text.chars().count())
        .max()
        .unwrap_or(0);
    ((header.max(max_cell) as f32) * char_w + 16.0).clamp(60.0, 340.0)
}

fn sort_dir(sort: &Document, field: &str) -> i32 {
    match sort.get(field) {
        Some(Bson::Int32(n)) => n.signum(),
        Some(Bson::Int64(n)) => n.signum() as i32,
        Some(Bson::Double(n)) => n.signum() as i32,
        _ => 0,
    }
}

pub struct GridOpts<'a> {
    pub sort: &'a Document,
    pub row_offset: u64,
    pub editable: bool,
}

/// Mostra a tabela. Retorna as ações pedidas pelo usuário.
pub fn show_table(ui: &mut Ui, id: Id, data: &GridData, state: &mut GridState, opts: GridOpts<'_>) -> Vec<GridAction> {
    let mut actions = Vec::new();
    let visible: Vec<usize> = (0..data.columns.len())
        .filter(|c| !state.hidden.contains(&data.columns[*c]))
        .collect();
    let body_h = ui.text_style_height(&TextStyle::Body);
    let row_h = body_h + 8.0;
    let char_w = ui.ctx().fonts_mut(|f| f.glyph_width(&TextStyle::Body.resolve(ui.style()), '0')).max(6.0);

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    state.generation.hash(&mut hasher);
    for c in &visible {
        data.columns[*c].hash(&mut hasher);
    }
    let table_salt = hasher.finish();

    let mono_w = ui.ctx().fonts_mut(|f| f.glyph_width(&TextStyle::Monospace.resolve(ui.style()), '0'));
    let num_w = ((opts.row_offset + data.docs.len() as u64).to_string().len() as f32 * mono_w + 18.0).max(34.0);
    let hidden_count = state.hidden.len();
    let mut widths_out: Vec<f32> = Vec::new();
    let mut header_actions = Vec::new();
    let mut unhide_all = false;
    let mut hide_col: Option<String> = None;
    let mut reset_widths = false;

    egui::ScrollArea::horizontal()
        .id_salt(id.with("hscroll"))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut tb = TableBuilder::new(ui)
                .id_salt(table_salt)
                .striped(true)
                .resizable(true)
                .sense(Sense::click())
                .cell_layout(Layout::left_to_right(Align::Center))
                .auto_shrink([false, false])
                .min_scrolled_height(0.0)
                .max_scroll_height(f32::INFINITY)
                .animate_scrolling(false);
            if let Some(r) = state.scroll_to.take() {
                tb = tb.scroll_to_row(r, None);
            }
            tb = tb.column(Column::exact(num_w).resizable(false));
            for &c in &visible {
                let name = &data.columns[c];
                let w = state
                    .widths
                    .get(name)
                    .copied()
                    .unwrap_or_else(|| estimate_width(data, c, char_w));
                tb = tb.column(Column::initial(w).at_least(36.0).clip(true).resizable(true));
            }
            tb.header(row_h + 2.0, |mut header| {
                header.col(|ui| {
                    let r = ui.add(Label::weak("#"));
                    if hidden_count > 0 {
                        r.on_hover_text(format!("{hidden_count} coluna(s) oculta(s) — clique direito num cabeçalho para mostrar"));
                    }
                });
                for &c in &visible {
                    let name = &data.columns[c];
                    let dir = sort_dir(opts.sort, name);
                    let arrow = match dir {
                        1 => " ⏶",
                        -1 => " ⏷",
                        _ => "",
                    };
                    let (_, resp) = header.col(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(format!("{name}{arrow}")).strong())
                                .selectable(false)
                                .truncate(),
                        );
                    });
                    let resp = resp.on_hover_text(format!(
                        "{name}\nClique: ordenar (⏶ ➡ ⏷ ➡ sem ordem)\nShift+clique: adicionar à ordenação\nArraste a borda para ajustar a largura"
                    ));
                    if resp.clicked() {
                        let append = ui_shift(&resp);
                        let next = match dir {
                            0 => 1,
                            1 => -1,
                            _ => 0,
                        };
                        header_actions.push(GridAction::Sort {
                            field: name.clone(),
                            dir: next,
                            append,
                        });
                    }
                    resp.context_menu(|ui| {
                        if ui.button("⏶ Ordenar crescente").clicked() {
                            header_actions.push(GridAction::Sort { field: name.clone(), dir: 1, append: false });
                            ui.close();
                        }
                        if ui.button("⏷ Ordenar decrescente").clicked() {
                            header_actions.push(GridAction::Sort { field: name.clone(), dir: -1, append: false });
                            ui.close();
                        }
                        if dir != 0 && ui.button("Remover da ordenação").clicked() {
                            header_actions.push(GridAction::Sort { field: name.clone(), dir: 0, append: true });
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Ocultar coluna").clicked() {
                            hide_col = Some(name.clone());
                            ui.close();
                        }
                        if hidden_count > 0 && ui.button(format!("Mostrar todas ({hidden_count} ocultas)")).clicked() {
                            unhide_all = true;
                            ui.close();
                        }
                        if ui.button("Restaurar larguras").clicked() {
                            reset_widths = true;
                            ui.close();
                        }
                        if ui.button("Copiar nome do campo").clicked() {
                            ui.ctx().copy_text(name.clone());
                            ui.close();
                        }
                    });
                }
            })
            .body(|body| {
                widths_out = body.widths().to_vec();
                body.rows(row_h, data.docs.len(), |mut row| {
                    let i = row.index();
                    row.set_selected(state.selected == Some(i));
                    row.col(|ui| {
                        ui.add(Label::weak(&(opts.row_offset + i as u64 + 1).to_string()));
                    });
                    for &c in &visible {
                        let cell = &data.cells[i][c];
                        let col_name = &data.columns[c];
                        let (_, resp) = row.col(|ui| {
                            let text = if cell.kind == CellKind::Missing { "—" } else { cell.text.as_str() };
                            let mut rt = RichText::new(text).color(cell_color(cell.kind, ui));
                            if matches!(cell.kind, CellKind::Number | CellKind::ObjectId | CellKind::Date) {
                                rt = rt.monospace();
                            }
                            ui.add(egui::Label::new(rt).selectable(false).truncate().show_tooltip_when_elided(false));
                        });
                        if resp.double_clicked() {
                            actions.push(GridAction::EditCell(i, col_name.clone()));
                        }
                        if resp.clicked() || resp.secondary_clicked() {
                            state.selected = Some(i);
                        }
                        let doc = &data.docs[i];
                        let resp = if resp.hovered() && cell.kind != CellKind::Missing {
                            resp.on_hover_ui(|ui| {
                                ui.set_max_width(600.0);
                                if let Some(v) = doc.get(col_name) {
                                    ui.label(RichText::new(shell::type_name(v)).small().weak());
                                    let full = shell::to_shell_pretty(v);
                                    let shown: String = full.chars().take(4000).collect();
                                    editor::code_view(ui, &shown);
                                }
                            })
                        } else {
                            resp
                        };
                        resp.context_menu(|ui| {
                            state.selected = Some(i);
                            if opts.editable {
                                if ui.button("✏ Editar campo…").clicked() {
                                    actions.push(GridAction::EditCell(i, col_name.clone()));
                                    ui.close();
                                }
                                if ui.button("📝 Editar documento…").clicked() {
                                    actions.push(GridAction::OpenDocument(i));
                                    ui.close();
                                }
                            } else if ui.button("🔍 Ver documento").clicked() {
                                actions.push(GridAction::OpenDocument(i));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Copiar valor").clicked() {
                                actions.push(GridAction::CopyValue(i, col_name.clone()));
                                ui.close();
                            }
                            if ui.button("Copiar documento").clicked() {
                                actions.push(GridAction::CopyDocument(i));
                                ui.close();
                            }
                            if cell.kind != CellKind::Missing && ui.button("Filtrar por este valor").clicked() {
                                actions.push(GridAction::FilterBy(i, col_name.clone()));
                                ui.close();
                            }
                            if opts.editable {
                                ui.separator();
                                if ui.button("🗐 Clonar documento").clicked() {
                                    actions.push(GridAction::CloneDocument(i));
                                    ui.close();
                                }
                                if ui.button(RichText::new("🗑 Excluir documento").color(ui.visuals().error_fg_color)).clicked() {
                                    actions.push(GridAction::DeleteDocument(i));
                                    ui.close();
                                }
                            }
                        });
                    }
                    let resp = row.response();
                    if resp.clicked() {
                        state.selected = Some(i);
                    }
                });
            });
        });

    for (w, &c) in widths_out.iter().skip(1).zip(visible.iter()) {
        state.widths.insert(data.columns[c].clone(), *w);
    }
    if let Some(h) = hide_col {
        state.hidden.insert(h);
    }
    if unhide_all {
        state.hidden.clear();
    }
    if reset_widths {
        state.widths.clear();
        state.generation += 1;
    }
    actions.extend(header_actions);
    actions
}

fn ui_shift(resp: &egui::Response) -> bool {
    resp.ctx.input(|i| i.modifiers.shift)
}

struct Label;
impl Label {
    fn weak(text: &str) -> egui::Label {
        egui::Label::new(RichText::new(text).weak().monospace()).selectable(false)
    }
}

/// Lista de documentos em formato JSON (shell), com rolagem virtual.
pub fn show_json(ui: &mut Ui, id: Id, data: &GridData, state: &mut GridState, opts: GridOpts<'_>) -> Vec<GridAction> {
    let mut actions = Vec::new();
    let line_h = ui.text_style_height(&TextStyle::Monospace) + 1.0;
    let header_h = 26.0;
    let gap = 8.0;
    let heights: Vec<f32> = data
        .json_lines
        .iter()
        .map(|n| *n as f32 * line_h + header_h + 18.0)
        .collect();
    let mut tops = Vec::with_capacity(heights.len());
    let mut y = 0.0;
    for h in &heights {
        tops.push(y);
        y += h + gap;
    }
    let total_h = y;
    let mut scroll = egui::ScrollArea::both().id_salt(id.with("json")).auto_shrink([false, false]);
    if let Some(r) = state.scroll_to.take()
        && let Some(t) = tops.get(r) {
            scroll = scroll.vertical_scroll_offset(*t);
        }
    scroll.show_viewport(ui, |ui, viewport| {
        ui.set_height(total_h);
        let origin = ui.min_rect().min;
        let width = ui.available_width().max(400.0);
        let first = tops.partition_point(|t| *t + 1e5 < viewport.min.y).saturating_sub(1);
        for i in first..data.docs.len() {
            let top = tops[i];
            if top > viewport.max.y {
                break;
            }
            if top + heights[i] < viewport.min.y {
                continue;
            }
            let rect = egui::Rect::from_min_size(origin + egui::vec2(0.0, top), egui::vec2(width, heights[i]));
            let selected = state.selected == Some(i);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
            let frame = egui::Frame::group(child.style())
                .fill(if selected { child.visuals().selection.bg_fill.gamma_multiply(0.25) } else { child.visuals().extreme_bg_color })
                .inner_margin(egui::Margin::same(8));
            let resp = frame
                .show(&mut child, |ui| {
                    ui.set_min_width(width - 20.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("#{}", opts.row_offset + i as u64 + 1)).weak().monospace());
                        if opts.editable {
                            if ui.small_button("✏ Editar").clicked() {
                                actions.push(GridAction::OpenDocument(i));
                            }
                            if ui.small_button("🗐 Clonar").clicked() {
                                actions.push(GridAction::CloneDocument(i));
                            }
                        } else if ui.small_button("🔍 Ver").clicked() {
                            actions.push(GridAction::OpenDocument(i));
                        }
                        if ui.small_button("Copiar").clicked() {
                            actions.push(GridAction::CopyDocument(i));
                        }
                        if opts.editable && ui.small_button(RichText::new("🗑 Excluir").color(ui.visuals().error_fg_color)).clicked() {
                            actions.push(GridAction::DeleteDocument(i));
                        }
                    });
                    editor::code_view(ui, &data.json[i]);
                })
                .response;
            if resp.interact(Sense::click()).clicked() {
                state.selected = Some(i);
            }
        }
    });
    actions
}
