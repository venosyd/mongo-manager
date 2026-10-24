//! Conteúdo das abas: Documentos, Agregação, Esquema e Índices.

use crate::app::{ACCENT, ACCENT_DARK, App, Paged, PendingSelect, ResultView, View, ViewMode, fmt_bytes, fmt_num};
use crate::completion::EditorKind;
use crate::config::{HistoryEntry, SavedPipeline};
use crate::dialogs::{ConfirmAction, Dialog, ExportSource};
use crate::editor::{CodeEditor, code_view};
use crate::grid::{self, GridAction, GridData, GridOpts};
use crate::mongo::{self, FindSpec};
use crate::shell;
use bson::{Bson, Document, doc};
use egui::{Id, Key, Modifiers, RichText, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Docs,
    Agg,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PagerAction {
    First,
    Prev,
    Next,
    Last,
    Goto(u64),
    PageSize(u64),
    Refresh,
}

enum TabAction {
    RunFind,
    ResetQuery,
    Explain,
    Pager(Which, PagerAction),
    Grid(Which, GridAction),
    Insert,
    Import(bool),
    Export(Which),
    UpdateMany,
    DeleteMany,
    ApplyHistory(HistoryEntry),
    ToggleFavorite(HistoryEntry),
    ClearHistory,
    RunAgg,
    ExplainAgg,
    SavePipeline,
    LoadPipeline(String),
    DeletePipeline(usize),
    InsertStage(&'static str),
    RefreshIndexes,
    CreateIndex,
    DropIndex(String),
    Resample,
    RefreshAll,
}

fn as_f64(b: Option<&Bson>) -> Option<f64> {
    match b? {
        Bson::Int32(n) => Some(*n as f64),
        Bson::Int64(n) => Some(*n as f64),
        Bson::Double(n) => Some(*n),
        _ => None,
    }
}

/// Barra de paginação: "1–50 de 1.234  ⏮ ◀ [3] / 25 ▶ ⏭  [50 ⏷]".
fn pager(ui: &mut Ui, id: Id, p: &mut Paged) -> Option<PagerAction> {
    let mut action = None;
    let shown = p.result.as_ref().map(|r| r.grid.docs.len() as u64).unwrap_or(0);
    let start = p.page * p.page_size;
    let range = if shown == 0 {
        "0".to_string()
    } else {
        format!("{}–{}", fmt_num(start + 1), fmt_num(start + shown))
    };
    let total = match (p.total, p.counting) {
        (Some(t), _) => fmt_num(t),
        (None, true) => "…".into(),
        (None, false) => "?".into(),
    };
    let last = p.last_page();

    ui.spacing_mut().item_spacing.x = 4.0;
    egui::ComboBox::from_id_salt(id.with("ps"))
        .selected_text(format!("{} / pág", p.page_size))
        .width(88.0)
        .show_ui(ui, |ui| {
            for n in [10u64, 20, 50, 100, 200, 500, 1000, 2000, 5000] {
                if ui.selectable_label(p.page_size == n, n.to_string()).clicked() {
                    action = Some(PagerAction::PageSize(n));
                }
            }
        });
    if ui
        .add_enabled(last.is_some_and(|l| p.page < l), egui::Button::new("⏭"))
        .on_hover_text("Última página (Ctrl+End)")
        .on_disabled_hover_text(if p.total.is_none() { "Aguardando contagem…" } else { "Já está na última página" })
        .clicked()
    {
        action = Some(PagerAction::Last);
    }
    if ui
        .add_enabled(p.has_next(), egui::Button::new("▶"))
        .on_hover_text("Próxima página (Alt+➡)")
        .clicked()
    {
        action = Some(PagerAction::Next);
    }
    if let Some(l) = last {
        ui.label(RichText::new(format!("/ {}", fmt_num(l + 1))).weak());
    }
    let input_id = id.with("page_input");
    let focused = ui.memory(|m| m.has_focus(input_id));
    if !focused {
        p.page_input = (p.page + 1).to_string();
    }
    let r = ui.add(
        egui::TextEdit::singleline(&mut p.page_input)
            .id(input_id)
            .desired_width(48.0)
            .align(egui::Align2::CENTER_CENTER),
    );
    let r = r.on_hover_text("Digite a página e pressione Enter");
    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
        && let Ok(n) = p.page_input.trim().replace('.', "").parse::<u64>() {
            let target = n.max(1) - 1;
            let target = last.map_or(target, |l| target.min(l));
            action = Some(PagerAction::Goto(target));
        }
    if ui
        .add_enabled(p.page > 0, egui::Button::new("◀"))
        .on_hover_text("Página anterior (Alt+⬅)")
        .clicked()
    {
        action = Some(PagerAction::Prev);
    }
    if ui
        .add_enabled(p.page > 0, egui::Button::new("⏮"))
        .on_hover_text("Primeira página (Ctrl+Home)")
        .clicked()
    {
        action = Some(PagerAction::First);
    }
    ui.add_space(6.0);
    let label = ui.label(format!("{range} de {total}"));
    if let Some(e) = &p.count_error {
        label.on_hover_text(format!("Falha ao contar: {e}"));
    }
    if p.loading || p.counting {
        ui.add(egui::Spinner::new().size(14.0));
    }
    if ui.button("⟳").on_hover_text("Recarregar página (F5)").clicked() {
        action = Some(PagerAction::Refresh);
    }
    action
}

/// Navegação por teclado na tabela (quando nenhum campo de texto tem foco).
fn grid_keyboard(ui: &Ui, p: &mut Paged, editable: bool) -> Vec<TabAction> {
    let mut out = Vec::new();
    let n = p.result.as_ref().map(|r| r.grid.docs.len()).unwrap_or(0);
    let (down, up, pgdn, pgup, home, end, c_home, c_end, a_left, a_right, enter, del, copy, f5) = ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::PageDown),
            i.consume_key(Modifiers::NONE, Key::PageUp),
            i.consume_key(Modifiers::NONE, Key::Home),
            i.consume_key(Modifiers::NONE, Key::End),
            i.consume_key(Modifiers::COMMAND, Key::Home),
            i.consume_key(Modifiers::COMMAND, Key::End),
            i.consume_key(Modifiers::ALT, Key::ArrowLeft),
            i.consume_key(Modifiers::ALT, Key::ArrowRight),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Delete),
            i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
            i.consume_key(Modifiers::NONE, Key::F5),
        )
    });
    let sel = p.grid.selected;
    let select = |p: &mut Paged, i: usize| {
        p.grid.selected = Some(i);
        p.grid.scroll_to = Some(i);
    };
    if f5 {
        out.push(TabAction::Pager(Which::Docs, PagerAction::Refresh));
    }
    if c_home {
        p.pending_select = Some(PendingSelect::First);
        out.push(TabAction::Pager(Which::Docs, PagerAction::First));
    }
    if c_end {
        p.pending_select = Some(PendingSelect::Last);
        out.push(TabAction::Pager(Which::Docs, PagerAction::Last));
    }
    if a_left && p.page > 0 {
        out.push(TabAction::Pager(Which::Docs, PagerAction::Prev));
    }
    if a_right && p.has_next() {
        out.push(TabAction::Pager(Which::Docs, PagerAction::Next));
    }
    if n == 0 {
        return out;
    }
    if down {
        match sel {
            Some(s) if s + 1 >= n && p.has_next() => {
                p.pending_select = Some(PendingSelect::First);
                out.push(TabAction::Pager(Which::Docs, PagerAction::Next));
            }
            Some(s) => select(p, (s + 1).min(n - 1)),
            None => select(p, 0),
        }
    }
    if up {
        match sel {
            Some(0) if p.page > 0 => {
                p.pending_select = Some(PendingSelect::Last);
                out.push(TabAction::Pager(Which::Docs, PagerAction::Prev));
            }
            Some(s) => select(p, s.saturating_sub(1)),
            None => select(p, 0),
        }
    }
    if pgdn {
        select(p, sel.map_or(0, |s| (s + 20).min(n - 1)));
    }
    if pgup {
        select(p, sel.map_or(0, |s| s.saturating_sub(20)));
    }
    if home {
        select(p, 0);
    }
    if end {
        select(p, n - 1);
    }
    if let Some(s) = p.grid.selected.filter(|s| *s < n) {
        if enter {
            out.push(TabAction::Grid(Which::Docs, GridAction::OpenDocument(s)));
        }
        if del && editable {
            out.push(TabAction::Grid(Which::Docs, GridAction::DeleteDocument(s)));
        }
        if copy {
            out.push(TabAction::Grid(Which::Docs, GridAction::CopyDocument(s)));
        }
    }
    out
}

fn retarget(actions: Vec<TabAction>, which: Which) -> Vec<TabAction> {
    actions
        .into_iter()
        .map(|a| match a {
            TabAction::Pager(_, p) => TabAction::Pager(which, p),
            TabAction::Grid(_, g) => TabAction::Grid(which, g),
            other => other,
        })
        .collect()
}

fn results_area(ui: &mut Ui, id: Id, p: &mut Paged, sort: &Document, editable: bool, which: Which) -> Vec<TabAction> {
    let mut actions = Vec::new();
    if let Some(e) = &p.error {
        egui::Frame::new()
            .fill(ui.visuals().error_fg_color.gamma_multiply(0.12))
            .corner_radius(4)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(format!("⚠ {e}")).color(ui.visuals().error_fg_color)).wrap());
            });
    }
    let Some(result) = &p.result else {
        if p.loading {
            ui.centered_and_justified(|ui| ui.spinner());
        }
        return actions;
    };
    if result.grid.docs.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(if p.loading { "Carregando…" } else { "Nenhum documento encontrado." }).weak());
        });
        return actions;
    }
    let opts = GridOpts {
        sort,
        row_offset: p.page * p.page_size,
        editable,
    };
    let grid_actions = match p.mode {
        ViewMode::Table => grid::show_table(ui, id, &result.grid, &mut p.grid, opts),
        ViewMode::Json => grid::show_json(ui, id, &result.grid, &mut p.grid, opts),
    };
    actions.extend(grid_actions.into_iter().map(|g| TabAction::Grid(which, g)));
    actions
}

const PAGER_WIDTH: f32 = 560.0;

fn pager_row(ui: &mut Ui, id: Id, p: &mut Paged, which: Which, actions: &mut Vec<TabAction>) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(a) = pager(ui, id, p) {
            actions.push(TabAction::Pager(which, a));
        }
        if let Some(r) = &p.result {
            ui.label(RichText::new(format!("{} ms", r.elapsed_ms)).small().weak());
        }
    });
}

fn mode_toggle(ui: &mut Ui, p: &mut Paged) {
    ui.selectable_value(&mut p.mode, ViewMode::Table, "⊞ Tabela");
    ui.selectable_value(&mut p.mode, ViewMode::Json, "{ } JSON");
}

impl App {
    pub fn tab_content(&mut self, ui: &mut Ui, idx: usize) {
        let accent = if ui.visuals().dark_mode { ACCENT } else { ACCENT_DARK };
        let tab = &mut self.tabs[idx];
        let tab_id = tab.id;
        let mut actions = Vec::new();

        // Cabeçalho: nome + estatísticas + seletor de visão.
        ui.horizontal(|ui| {
            ui.label(RichText::new(&tab.conn_name).weak());
            ui.label(RichText::new("›").weak());
            ui.label(RichText::new(&tab.ns.db).weak());
            ui.label(RichText::new("›").weak());
            ui.label(RichText::new(&tab.ns.coll).strong().size(16.0));
            if tab.kind == mongo::CollKind::View {
                ui.label(RichText::new("VIEW").small().color(accent));
            }
            if let Some(s) = &tab.stats {
                let count = as_f64(s.get("count")).unwrap_or(0.0);
                let size = as_f64(s.get("size")).unwrap_or(0.0);
                let avg = as_f64(s.get("avgObjSize")).unwrap_or(0.0);
                let nidx = as_f64(s.get("nindexes")).unwrap_or(0.0);
                let idx_size = as_f64(s.get("totalIndexSize")).unwrap_or(0.0);
                let storage = as_f64(s.get("storageSize")).unwrap_or(0.0);
                ui.add_space(12.0);
                ui.label(
                    RichText::new(format!(
                        "{} docs · {} · média {} · {} índice(s) {}",
                        fmt_num(count as u64),
                        fmt_bytes(size),
                        fmt_bytes(avg),
                        nidx,
                        fmt_bytes(idx_size)
                    ))
                    .small()
                    .weak(),
                )
                .on_hover_text(format!("Armazenamento em disco: {}", fmt_bytes(storage)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⟳").on_hover_text("Atualizar tudo").clicked() {
                    actions.push(TabAction::RefreshAll);
                }
                for (v, label) in [
                    (View::Indexes, "Índices"),
                    (View::Schema, "Esquema"),
                    (View::Aggregation, "Agregação"),
                    (View::Documents, "Documentos"),
                ] {
                    let selected = tab.view == v;
                    let text = if selected { RichText::new(label).strong().color(accent) } else { RichText::new(label) };
                    if ui.selectable_label(selected, text).clicked() {
                        tab.view = v;
                        if v == View::Indexes && tab.idx.list.is_none() && !tab.idx.loading {
                            actions.push(TabAction::RefreshIndexes);
                        }
                    }
                }
            });
        });
        ui.separator();

        let view = tab.view;
        actions.extend(match view {
            View::Documents => self.documents_view(ui, idx),
            View::Aggregation => self.aggregation_view(ui, idx),
            View::Schema => self.schema_view(ui, idx),
            View::Indexes => self.indexes_view(ui, idx),
        });

        for a in actions {
            self.apply_tab_action(tab_id, a, ui.ctx());
        }
    }

    fn documents_view(&mut self, ui: &mut Ui, idx: usize) -> Vec<TabAction> {
        let mut actions = Vec::new();
        let no_dialogs = self.dialogs.is_empty();
        let tab = &mut self.tabs[idx];
        let tab_id = tab.id;
        let editable = tab.editable();
        let history_key = tab.history_key();
        let history = self.config.history.get(&history_key).cloned().unwrap_or_default();
        let fields = &tab.fields;
        let d = &mut tab.docs;

        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
            ui.memory_mut(|m| m.request_focus(Id::unique(("filter", tab_id))));
        }

        // Barra de consulta.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filtro").strong());
            let w = (ui.available_width() - 400.0).max(200.0);
            let out = CodeEditor::new(Id::unique(("filter", tab_id)), &mut d.filter, EditorKind::Filter, fields)
                .submit_on_enter(true)
                .hint("{ campo: valor }   ·   Ctrl+Espaço: sugestões   ·   Enter: buscar")
                .width(w)
                .show(ui);
            if out.submit {
                actions.push(TabAction::RunFind);
            }
            if ui
                .button(RichText::new("▶ Buscar").strong())
                .on_hover_text("Executar consulta (Enter no filtro)")
                .clicked()
            {
                actions.push(TabAction::RunFind);
            }
            if ui.button("Limpar").on_hover_text("Limpar filtro, projeção e ordenação").clicked() {
                actions.push(TabAction::ResetQuery);
            }
            ui.toggle_value(&mut d.show_options, "Opções").on_hover_text("Projeção, ordenação e hint");
            ui.menu_button("🕘", |ui| {
                ui.set_min_width(420.0);
                if history.is_empty() {
                    ui.label(RichText::new("Nenhuma consulta no histórico.").weak());
                }
                let mut favs: Vec<&HistoryEntry> = history.iter().filter(|h| h.favorite).collect();
                let rest: Vec<&HistoryEntry> = history.iter().filter(|h| !h.favorite).collect();
                if !favs.is_empty() {
                    ui.label(RichText::new("★ Favoritas").small().strong());
                }
                favs.extend(rest);
                let mut shown_rest_header = false;
                for h in favs {
                    if !h.favorite && !shown_rest_header {
                        ui.label(RichText::new("Recentes").small().strong());
                        shown_rest_header = true;
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .small_button(if h.favorite { "★" } else { "☆" })
                            .on_hover_text("Favoritar")
                            .clicked()
                        {
                            actions.push(TabAction::ToggleFavorite(h.clone()));
                        }
                        let mut text = h.filter.clone();
                        if !h.sort.is_empty() {
                            text.push_str(&format!("  sort: {}", h.sort));
                        }
                        if !h.project.is_empty() {
                            text.push_str(&format!("  proj: {}", h.project));
                        }
                        let text: String = text.chars().take(120).collect();
                        if ui.button(RichText::new(text).monospace()).clicked() {
                            actions.push(TabAction::ApplyHistory(h.clone()));
                            ui.close();
                        }
                    });
                }
                if !history.is_empty() {
                    ui.separator();
                    if ui.button("Limpar histórico (mantém favoritas)").clicked() {
                        actions.push(TabAction::ClearHistory);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Histórico de consultas");
            if ui.button("Explain").on_hover_text("Plano de execução da consulta").clicked() {
                actions.push(TabAction::Explain);
            }
        });
        if d.show_options {
            egui::Grid::new(("opts", tab_id)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                let w = ui.available_width() - 100.0;
                ui.label("Projeção");
                if CodeEditor::new(Id::unique(("project", tab_id)), &mut d.project, EditorKind::Projection, fields)
                    .submit_on_enter(true)
                    .hint("{ campo: 1, _id: 0 }")
                    .width(w)
                    .show(ui)
                    .submit
                {
                    actions.push(TabAction::RunFind);
                }
                ui.end_row();
                ui.label("Ordenação");
                if CodeEditor::new(Id::unique(("sort", tab_id)), &mut d.sort, EditorKind::Sort, fields)
                    .submit_on_enter(true)
                    .hint("{ campo: -1 }   (ou clique no cabeçalho da coluna)")
                    .width(w)
                    .show(ui)
                    .submit
                {
                    actions.push(TabAction::RunFind);
                }
                ui.end_row();
                ui.label("Hint");
                if CodeEditor::new(Id::unique(("hint", tab_id)), &mut d.hint, EditorKind::IndexKeys, fields)
                    .submit_on_enter(true)
                    .hint("{ campo: 1 } ou \"nome_do_indice\"")
                    .width(w)
                    .show(ui)
                    .submit
                {
                    actions.push(TabAction::RunFind);
                }
                ui.end_row();
            });
        }
        ui.add_space(2.0);

        // Barra de ferramentas + paginação (que desce para outra linha se faltar espaço).
        let mut pager_below = false;
        ui.horizontal(|ui| {
            if editable {
                ui.menu_button("➕ Adicionar", |ui| {
                    if ui.button("Inserir documento…").clicked() {
                        actions.push(TabAction::Insert);
                        ui.close();
                    }
                    if ui.button("Importar arquivo JSON…").clicked() {
                        actions.push(TabAction::Import(false));
                        ui.close();
                    }
                    if ui.button("Importar arquivo CSV…").clicked() {
                        actions.push(TabAction::Import(true));
                        ui.close();
                    }
                });
            }
            ui.menu_button("⚙ Ações", |ui| {
                if ui.button("⬇ Exportar resultado…").clicked() {
                    actions.push(TabAction::Export(Which::Docs));
                    ui.close();
                }
                if editable {
                    ui.separator();
                    if ui.button("Atualizar vários (updateMany)…").clicked() {
                        actions.push(TabAction::UpdateMany);
                        ui.close();
                    }
                    if ui.button("Excluir vários (deleteMany)…").clicked() {
                        actions.push(TabAction::DeleteMany);
                        ui.close();
                    }
                }
            });
            ui.separator();
            mode_toggle(ui, &mut d.paged);
            ui.toggle_value(&mut d.show_detail, "🗗")
                .on_hover_text("Painel lateral com o documento selecionado");
            if ui.available_width() >= PAGER_WIDTH {
                pager_row(ui, Id::unique(("pager", tab_id)), &mut d.paged, Which::Docs, &mut actions);
            } else {
                pager_below = true;
            }
        });
        if pager_below {
            ui.horizontal(|ui| pager_row(ui, Id::unique(("pager", tab_id)), &mut d.paged, Which::Docs, &mut actions));
        }

        if no_dialogs && ui.memory(|m| m.focused().is_none()) {
            actions.extend(grid_keyboard(ui, &mut d.paged, editable));
        }

        // Painel de detalhes.
        if d.show_detail {
            let selected = d
                .paged
                .grid
                .selected
                .and_then(|s| d.paged.result.as_ref().and_then(|r| r.grid.docs.get(s).map(|doc| (s, doc))));
            egui::Panel::right(Id::unique(("detail", tab_id)))
                .resizable(true)
                .default_size(380.0)
                .size_range(200.0..=1200.0)
                .show(ui, |ui| match selected {
                    Some((i, doc)) => {
                        ui.horizontal(|ui| {
                            if editable && ui.button("✏ Editar").clicked() {
                                actions.push(TabAction::Grid(Which::Docs, GridAction::OpenDocument(i)));
                            }
                            if editable && ui.button("🗐 Clonar").clicked() {
                                actions.push(TabAction::Grid(Which::Docs, GridAction::CloneDocument(i)));
                            }
                            if ui.button("Copiar").clicked() {
                                actions.push(TabAction::Grid(Which::Docs, GridAction::CopyDocument(i)));
                            }
                            if editable
                                && ui
                                    .button(RichText::new("🗑").color(ui.visuals().error_fg_color))
                                    .on_hover_text("Excluir documento")
                                    .clicked()
                            {
                                actions.push(TabAction::Grid(Which::Docs, GridAction::DeleteDocument(i)));
                            }
                        });
                        ui.separator();
                        let text = d.paged.result.as_ref().map(|r| r.grid.json[i].clone()).unwrap_or_default();
                        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                            let _ = doc;
                            code_view(ui, &text);
                        });
                    }
                    None => {
                        ui.label(RichText::new("Selecione um documento na tabela.").weak());
                    }
                });
        }

        let sort = d.applied_sort.clone();
        actions.extend(results_area(ui, Id::unique(("docs", tab_id)), &mut d.paged, &sort, editable, Which::Docs));
        actions
    }

    fn aggregation_view(&mut self, ui: &mut Ui, idx: usize) -> Vec<TabAction> {
        let mut actions = Vec::new();
        let no_dialogs = self.dialogs.is_empty();
        let tab = &mut self.tabs[idx];
        let tab_id = tab.id;
        let key = tab.history_key();
        let saved = self.config.pipelines.get(&key).cloned().unwrap_or_default();
        let fields = &tab.fields;
        let a = &mut tab.agg;

        ui.horizontal(|ui| {
            if ui.button(RichText::new("▶ Executar").strong()).on_hover_text("Ctrl+Enter").clicked() {
                actions.push(TabAction::RunAgg);
            }
            if ui.button("Explain").clicked() {
                actions.push(TabAction::ExplainAgg);
            }
            ui.menu_button("➕ Estágio", |ui| {
                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    for (name, snippet, desc) in crate::completion::STAGES {
                        if ui
                            .button(name.to_string())
                            .on_hover_text(*desc)
                            .clicked()
                        {
                            actions.push(TabAction::InsertStage(snippet));
                            ui.close();
                        }
                    }
                });
            });
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut a.save_name).hint_text("nome do pipeline").desired_width(160.0));
            if ui.button("💾 Salvar").clicked() {
                actions.push(TabAction::SavePipeline);
            }
            ui.menu_button(format!("📂 Salvos ({})", saved.len()), |ui| {
                ui.set_min_width(300.0);
                if saved.is_empty() {
                    ui.label(RichText::new("Nenhum pipeline salvo para esta coleção.").weak());
                }
                for (i, s) in saved.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.button(&s.name).on_hover_text(&s.pipeline).clicked() {
                            actions.push(TabAction::LoadPipeline(s.pipeline.clone()));
                            a.save_name = s.name.clone();
                            ui.close();
                        }
                        if ui.small_button("🗑").on_hover_text("Excluir").clicked() {
                            actions.push(TabAction::DeletePipeline(i));
                        }
                    });
                }
            });
            if ui.button("Copiar").on_hover_text("Copiar pipeline").clicked() {
                ui.ctx().copy_text(a.pipeline.clone());
            }
        });
        let avail = ui.available_height();
        egui::Frame::new()
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(4)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("pipe_scroll", tab_id))
                    .max_height((avail * 0.42).max(120.0))
                    .show(ui, |ui| {
                        let out = CodeEditor::new(Id::unique(("pipeline", tab_id)), &mut a.pipeline, EditorKind::Pipeline, fields)
                            .rows(8)
                            .hint("[ { $match: {} }, { $group: { _id: \"$campo\", total: { $sum: 1 } } } ]")
                            .show(ui);
                        if out.submit {
                            actions.push(TabAction::RunAgg);
                        }
                    });
            });
        if a.writes {
            ui.label(
                RichText::new("⚠ Este pipeline termina em $out/$merge: executá-lo grava dados.")
                    .color(ui.visuals().warn_fg_color),
            );
        }
        ui.horizontal(|ui| {
            if ui.button("⬇ Exportar").on_hover_text("Exportar todo o resultado").clicked() {
                actions.push(TabAction::Export(Which::Agg));
            }
            ui.separator();
            mode_toggle(ui, &mut a.paged);
            if a.applied.is_some() {
                pager_row(ui, Id::unique(("aggpager", tab_id)), &mut a.paged, Which::Agg, &mut actions);
            }
        });
        if no_dialogs && ui.memory(|m| m.focused().is_none()) {
            actions.extend(retarget(grid_keyboard(ui, &mut a.paged, false), Which::Agg));
        }
        if a.applied.is_none() && a.paged.result.is_none() && a.paged.error.is_none() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Escreva o pipeline e clique em Executar (Ctrl+Enter).").weak());
                ui.label(RichText::new("Ctrl+Espaço sugere estágios, operadores e campos ($campo).").weak());
            });
        }
        actions.extend(results_area(ui, Id::unique(("agg", tab_id)), &mut a.paged, &Document::new(), false, Which::Agg));
        actions
    }

    fn schema_view(&mut self, ui: &mut Ui, idx: usize) -> Vec<TabAction> {
        let mut actions = Vec::new();
        let sample_size = self.config.settings.sample_size;
        let tab = &mut self.tabs[idx];
        ui.horizontal(|ui| {
            if ui.button("⟳ Analisar novamente").clicked() {
                actions.push(TabAction::Resample);
            }
            ui.label("Amostra:");
            let mut s = sample_size;
            if ui.add(egui::DragValue::new(&mut s).range(10..=100_000).speed(10)).changed() {
                self.config.settings.sample_size = s;
            }
            ui.label(RichText::new(format!("{} documentos analisados · {} campos", tab.schema.sampled, tab.field_info.len())).weak());
            if tab.schema.loading {
                ui.spinner();
            }
        });
        if let Some(e) = &tab.schema.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        ui.separator();
        let total = tab.schema.sampled.max(1) as f32;
        let accent = if ui.visuals().dark_mode { ACCENT } else { ACCENT_DARK };
        egui_extras::TableBuilder::new(ui)
            .id_salt(("schema", tab.id))
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::initial(280.0).at_least(80.0).clip(true))
            .column(egui_extras::Column::initial(170.0).at_least(80.0))
            .column(egui_extras::Column::remainder())
            .header(26.0, |mut h| {
                h.col(|ui| {
                    ui.strong("Campo");
                });
                h.col(|ui| {
                    ui.strong("Presença");
                });
                h.col(|ui| {
                    ui.strong("Tipos");
                });
            })
            .body(|body| {
                body.rows(24.0, tab.field_info.len(), |mut row| {
                    let f = &tab.field_info[row.index()];
                    let depth = f.path.matches('.').count();
                    row.col(|ui| {
                        ui.add_space(depth as f32 * 14.0);
                        let name = f.path.rsplit('.').next().unwrap_or(&f.path);
                        ui.label(RichText::new(name).monospace()).on_hover_text(&f.path);
                    });
                    row.col(|ui| {
                        let frac = (f.count as f32 / total).min(1.0);
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_width(110.0)
                                .fill(accent.gamma_multiply(0.7))
                                .text(format!("{:.0}%", frac * 100.0)),
                        );
                    });
                    row.col(|ui| {
                        let sum: usize = f.types.iter().map(|t| t.1).sum::<usize>().max(1);
                        for (t, n) in &f.types {
                            ui.label(
                                RichText::new(format!("{t} {:.0}%", *n as f32 * 100.0 / sum as f32))
                                    .small()
                                    .background_color(ui.visuals().faint_bg_color),
                            );
                        }
                    });
                });
            });
        actions
    }

    fn indexes_view(&mut self, ui: &mut Ui, idx: usize) -> Vec<TabAction> {
        let mut actions = Vec::new();
        let tab = &mut self.tabs[idx];
        let editable = tab.editable();
        ui.horizontal(|ui| {
            if editable && ui.button("➕ Criar índice…").clicked() {
                actions.push(TabAction::CreateIndex);
            }
            if ui.button("⟳ Atualizar").clicked() {
                actions.push(TabAction::RefreshIndexes);
            }
            if tab.idx.loading {
                ui.spinner();
            }
        });
        if let Some(e) = &tab.idx.error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        ui.separator();
        let Some(list) = &tab.idx.list else { return actions };
        egui_extras::TableBuilder::new(ui)
            .id_salt(("indexes", tab.id))
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::initial(200.0).at_least(80.0).clip(true))
            .column(egui_extras::Column::initial(280.0).at_least(80.0).clip(true))
            .column(egui_extras::Column::initial(240.0).at_least(80.0).clip(true))
            .column(egui_extras::Column::initial(90.0))
            .column(egui_extras::Column::initial(90.0))
            .column(egui_extras::Column::remainder())
            .header(26.0, |mut h| {
                for t in ["Nome", "Chaves", "Propriedades", "Tamanho", "Uso (ops)", ""] {
                    h.col(|ui| {
                        ui.strong(t);
                    });
                }
            })
            .body(|body| {
                body.rows(26.0, list.indexes.len(), |mut row| {
                    let ix = &list.indexes[row.index()];
                    let name = ix.get_str("name").unwrap_or("?").to_string();
                    row.col(|ui| {
                        ui.label(RichText::new(&name).monospace());
                    });
                    row.col(|ui| {
                        let keys = ix.get_document("key").map(shell::doc_to_shell_compact).unwrap_or_default();
                        ui.label(RichText::new(&keys).monospace()).on_hover_text(&keys);
                    });
                    row.col(|ui| {
                        let mut props = Vec::new();
                        if ix.get_bool("unique").unwrap_or(false) {
                            props.push("ÚNICO".to_string());
                        }
                        if ix.get_bool("sparse").unwrap_or(false) {
                            props.push("SPARSE".to_string());
                        }
                        if let Some(ttl) = as_f64(ix.get("expireAfterSeconds")) {
                            props.push(format!("TTL {ttl}s"));
                        }
                        if ix.contains_key("partialFilterExpression") {
                            props.push("PARCIAL".to_string());
                        }
                        if ix.get_bool("hidden").unwrap_or(false) {
                            props.push("OCULTO".to_string());
                        }
                        if ix.contains_key("collation") {
                            props.push("COLLATION".to_string());
                        }
                        if let Ok(k) = ix.get_document("key") {
                            for v in k.values() {
                                if let Bson::String(s) = v {
                                    props.push(s.to_uppercase());
                                }
                            }
                        }
                        let mut details = ix.clone();
                        details.remove("v");
                        ui.label(RichText::new(props.join(" · ")).small())
                            .on_hover_text(shell::doc_to_shell_pretty(&details));
                    });
                    row.col(|ui| {
                        if let Some(sz) = list.sizes.get(&name) {
                            ui.label(fmt_bytes(*sz as f64));
                        }
                    });
                    row.col(|ui| {
                        if let Some(n) = list.usage.get(&name) {
                            ui.label(fmt_num(*n as u64));
                        }
                    });
                    row.col(|ui| {
                        if editable
                            && name != "_id_"
                            && ui
                                .small_button(RichText::new("🗑 Excluir").color(ui.visuals().error_fg_color))
                                .clicked()
                        {
                            actions.push(TabAction::DropIndex(name.clone()));
                        }
                    });
                });
            });
        actions
    }

    // -----------------------------------------------------------------------
    // Ações

    fn apply_tab_action(&mut self, tab_id: u64, action: TabAction, ctx: &egui::Context) {
        match action {
            TabAction::RunFind => self.run_find(tab_id),
            TabAction::ResetQuery => {
                if let Some(t) = self.tab_mut(tab_id) {
                    t.docs.filter.clear();
                    t.docs.project.clear();
                    t.docs.sort.clear();
                    t.docs.hint.clear();
                }
                self.run_find(tab_id);
            }
            TabAction::Explain => self.explain_find(tab_id),
            TabAction::Pager(which, a) => self.apply_pager(tab_id, which, a),
            TabAction::Grid(which, g) => self.apply_grid_action(tab_id, which, g, ctx),
            TabAction::Insert => self.open_dialog(Dialog::insert(tab_id, "{\n  \n}".into())),
            TabAction::Import(csv) => self.import_file(tab_id, csv),
            TabAction::Export(which) => {
                let source = match which {
                    Which::Docs => ExportSource::Query,
                    Which::Agg => ExportSource::Aggregation,
                };
                self.open_dialog(Dialog::export(tab_id, source));
            }
            TabAction::UpdateMany => {
                let filter = self.tab(tab_id).map(|t| t.docs.filter.clone()).unwrap_or_default();
                self.open_dialog(Dialog::update_many(tab_id, filter));
            }
            TabAction::DeleteMany => {
                let filter = self.tab(tab_id).map(|t| t.docs.filter.clone()).unwrap_or_default();
                self.open_dialog(Dialog::delete_many(tab_id, filter));
            }
            TabAction::ApplyHistory(h) => {
                if let Some(t) = self.tab_mut(tab_id) {
                    t.docs.filter = h.filter;
                    t.docs.project = h.project.clone();
                    t.docs.sort = h.sort.clone();
                    if !h.project.is_empty() || !h.sort.is_empty() {
                        t.docs.show_options = true;
                    }
                }
                self.run_find(tab_id);
            }
            TabAction::ToggleFavorite(h) => {
                let Some(key) = self.tab(tab_id).map(|t| t.history_key()) else { return };
                if let Some(list) = self.config.history.get_mut(&key)
                    && let Some(e) = list
                        .iter_mut()
                        .find(|e| e.filter == h.filter && e.sort == h.sort && e.project == h.project)
                    {
                        e.favorite = !e.favorite;
                    }
                self.save_config();
            }
            TabAction::ClearHistory => {
                let Some(key) = self.tab(tab_id).map(|t| t.history_key()) else { return };
                if let Some(list) = self.config.history.get_mut(&key) {
                    list.retain(|e| e.favorite);
                }
                self.save_config();
            }
            TabAction::RunAgg => self.run_agg(tab_id),
            TabAction::ExplainAgg => self.explain_agg(tab_id),
            TabAction::SavePipeline => {
                let Some(t) = self.tab(tab_id) else { return };
                let key = t.history_key();
                let name = if t.agg.save_name.trim().is_empty() {
                    format!("pipeline {}", chrono::Local::now().format("%d/%m %H:%M"))
                } else {
                    t.agg.save_name.trim().to_string()
                };
                let pipeline = t.agg.pipeline.clone();
                let list = self.config.pipelines.entry(key).or_default();
                match list.iter_mut().find(|p| p.name == name) {
                    Some(p) => p.pipeline = pipeline,
                    None => list.push(SavedPipeline { name: name.clone(), pipeline }),
                }
                self.save_config();
                self.toast_ok(format!("Pipeline \"{name}\" salvo."));
            }
            TabAction::LoadPipeline(p) => {
                if let Some(t) = self.tab_mut(tab_id) {
                    t.agg.pipeline = p;
                }
            }
            TabAction::DeletePipeline(i) => {
                let Some(key) = self.tab(tab_id).map(|t| t.history_key()) else { return };
                if let Some(list) = self.config.pipelines.get_mut(&key)
                    && i < list.len() {
                        list.remove(i);
                    }
                self.save_config();
            }
            TabAction::InsertStage(snippet) => {
                if let Some(t) = self.tab_mut(tab_id) {
                    let stage = format!("{{ {} }}", snippet.replace(crate::completion::CARET, ""));
                    t.agg.pipeline = append_stage(&t.agg.pipeline, &stage);
                }
            }
            TabAction::RefreshIndexes => self.load_indexes(tab_id),
            TabAction::CreateIndex => self.open_dialog(Dialog::create_index(tab_id)),
            TabAction::DropIndex(name) => self.open_dialog(Dialog::confirm(
                "Excluir índice",
                format!("Excluir o índice \"{name}\"?"),
                "Excluir índice",
                ConfirmAction::DropIndex { tab: tab_id, name },
            )),
            TabAction::Resample => {
                self.save_config();
                self.sample_fields(tab_id);
            }
            TabAction::RefreshAll => {
                self.reload_docs(tab_id, true);
                self.sample_fields(tab_id);
                self.load_stats(tab_id);
                if self.tab(tab_id).is_some_and(|t| t.idx.list.is_some()) {
                    self.load_indexes(tab_id);
                }
            }
        }
    }

    fn apply_pager(&mut self, tab_id: u64, which: Which, a: PagerAction) {
        let page_size_default = &mut self.config.settings.page_size;
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else { return };
        let p = match which {
            Which::Docs => &mut tab.docs.paged,
            Which::Agg => &mut tab.agg.paged,
        };
        let mut recount = false;
        match a {
            PagerAction::First => p.page = 0,
            PagerAction::Prev => p.page = p.page.saturating_sub(1),
            PagerAction::Next => p.page += 1,
            PagerAction::Last => match p.last_page() {
                Some(l) => p.page = l,
                None => {
                    // Ainda contando: vai para o fim assim que a contagem chegar.
                    p.want_last = true;
                    return;
                }
            },
            PagerAction::Goto(n) => p.page = n,
            PagerAction::PageSize(n) => {
                // Mantém o primeiro documento visível na nova página.
                let first = p.page * p.page_size;
                p.page_size = n;
                p.page = first / n;
                if which == Which::Docs {
                    *page_size_default = n;
                }
            }
            PagerAction::Refresh => recount = true,
        }
        match which {
            Which::Docs => {
                if tab.docs.applied.is_none() {
                    self.run_find(tab_id);
                } else {
                    self.reload_docs(tab_id, recount);
                }
                if matches!(a, PagerAction::PageSize(_)) {
                    self.save_config();
                }
            }
            Which::Agg => {
                if recount {
                    self.start_agg_count(tab_id);
                }
                self.load_agg_page(tab_id);
            }
        }
    }

    pub fn reload_docs(&mut self, tab_id: u64, recount: bool) {
        if recount {
            self.start_count(tab_id);
        }
        self.load_docs_page(tab_id);
    }

    fn apply_grid_action(&mut self, tab_id: u64, which: Which, g: GridAction, ctx: &egui::Context) {
        let Some(tab) = self.tab(tab_id) else { return };
        let editable = tab.editable() && which == Which::Docs;
        let p = match which {
            Which::Docs => &tab.docs.paged,
            Which::Agg => &tab.agg.paged,
        };
        let doc_at = |i: usize| p.result.as_ref().and_then(|r| r.grid.docs.get(i)).cloned();
        match g {
            GridAction::Sort { field, dir, append } => {
                if which != Which::Docs {
                    return;
                }
                let mut sort = if append {
                    shell::parse_document(&tab.docs.sort).unwrap_or_default()
                } else {
                    Document::new()
                };
                if dir == 0 {
                    sort.remove(&field);
                } else {
                    sort.insert(field, dir);
                }
                let text = if sort.is_empty() { String::new() } else { shell::doc_to_shell_compact(&sort) };
                if let Some(t) = self.tab_mut(tab_id) {
                    t.docs.sort = text;
                }
                self.run_find(tab_id);
            }
            GridAction::OpenDocument(i) => {
                if let Some(d) = doc_at(i) {
                    self.open_dialog(Dialog::edit_document(tab_id, &d, !editable));
                }
            }
            GridAction::EditCell(i, col) => {
                let Some(d) = doc_at(i) else { return };
                if !editable {
                    self.open_dialog(Dialog::edit_document(tab_id, &d, true));
                    return;
                }
                let Some(id) = d.get("_id").cloned() else {
                    self.toast_err("Documento sem _id (talvez excluído pela projeção): não é possível editar.");
                    return;
                };
                if col == "_id" {
                    self.toast_err("O campo _id não pode ser alterado.");
                    return;
                }
                self.open_dialog(Dialog::edit_field(tab_id, id, col.clone(), d.get(&col).cloned()));
            }
            GridAction::CopyValue(i, col) => {
                if let Some(v) = doc_at(i).and_then(|d| d.get(&col).cloned()) {
                    let text = match &v {
                        Bson::String(s) => s.clone(),
                        Bson::ObjectId(o) => o.to_hex(),
                        other => shell::to_shell_pretty(other),
                    };
                    ctx.copy_text(text);
                    self.toast_ok("Valor copiado.");
                }
            }
            GridAction::CopyDocument(i) => {
                if let Some(d) = doc_at(i) {
                    ctx.copy_text(shell::doc_to_shell_pretty(&d));
                    self.toast_ok("Documento copiado.");
                }
            }
            GridAction::CloneDocument(i) => {
                if let Some(mut d) = doc_at(i) {
                    d.remove("_id");
                    self.open_dialog(Dialog::insert(tab_id, shell::doc_to_shell_pretty(&d)));
                }
            }
            GridAction::DeleteDocument(i) => {
                let Some(d) = doc_at(i) else { return };
                let Some(id) = d.get("_id").cloned() else {
                    self.toast_err("Documento sem _id: não é possível excluir.");
                    return;
                };
                self.open_dialog(Dialog::confirm(
                    "Excluir documento",
                    format!("Excluir o documento com _id {}?", shell::to_shell_compact(&id)),
                    "Excluir",
                    ConfirmAction::DeleteDocument { tab: tab_id, id },
                ));
            }
            GridAction::FilterBy(i, col) => {
                let Some(v) = doc_at(i).and_then(|d| d.get(&col).cloned()) else { return };
                let cond = shell::doc_to_shell_compact(&doc! { col: v });
                match which {
                    Which::Docs => {
                        if let Some(t) = self.tab_mut(tab_id) {
                            t.docs.filter = cond;
                        }
                        self.run_find(tab_id);
                    }
                    Which::Agg => {
                        if let Some(t) = self.tab_mut(tab_id) {
                            t.agg.pipeline = append_stage(&t.agg.pipeline, &format!("{{ $match: {cond} }}"));
                        }
                        self.run_agg(tab_id);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Consultas

    fn parse_find_spec(&self, tab_id: u64) -> Result<(FindSpec, Document), String> {
        let tab = self.tab(tab_id).ok_or("aba fechada")?;
        let d = &tab.docs;
        let filter = shell::parse_document(&d.filter).map_err(|e| format!("Filtro: {e}"))?;
        let projection = shell::parse_document(&d.project).map_err(|e| format!("Projeção: {e}"))?;
        let user_sort = shell::parse_document(&d.sort).map_err(|e| format!("Ordenação: {e}"))?;
        let hint = if d.hint.trim().is_empty() {
            None
        } else {
            Some(shell::parse_value(&d.hint).map_err(|e| format!("Hint: {e}"))?)
        };
        let mut sort = user_sort.clone();
        // Desempate por _id: paginação estável e leitura reversa consistente.
        if !sort.is_empty() && !sort.contains_key("_id") && sort.values().all(|v| matches!(v, Bson::Int32(_) | Bson::Int64(_) | Bson::Double(_))) {
            sort.insert("_id", 1);
        }
        Ok((
            FindSpec {
                filter,
                projection: (!projection.is_empty()).then_some(projection),
                sort: (!sort.is_empty()).then_some(sort),
                hint,
                skip: 0,
                limit: d.paged.page_size,
                max_time_ms: self.config.settings.max_time_ms,
            },
            user_sort,
        ))
    }

    pub fn run_find(&mut self, tab_id: u64) {
        let parsed = self.parse_find_spec(tab_id);
        let Some(tab) = self.tab_mut(tab_id) else { return };
        let (spec, user_sort) = match parsed {
            Ok(v) => v,
            Err(e) => {
                tab.docs.paged.error = Some(e);
                return;
            }
        };
        let entry = HistoryEntry {
            filter: tab.docs.filter.trim().to_string(),
            project: tab.docs.project.trim().to_string(),
            sort: tab.docs.sort.trim().to_string(),
            favorite: false,
        };
        let key = tab.history_key();
        tab.docs.applied = Some(spec);
        tab.docs.applied_sort = user_sort;
        tab.docs.paged.page = 0;
        tab.docs.paged.total = None;
        tab.docs.paged.error = None;
        tab.docs.paged.grid.selected = None;
        self.config.push_history(&key, entry);
        self.save_config();
        self.start_count(tab_id);
        self.load_docs_page(tab_id);
    }

    fn start_count(&mut self, tab_id: u64) {
        let Some(tab) = self.tab_mut(tab_id) else { return };
        let Some(spec) = tab.docs.applied.clone() else { return };
        let p = &mut tab.docs.paged;
        p.count_seq += 1;
        p.counting = true;
        p.count_error = None;
        let seq = p.count_seq;
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        self.backend.spawn(mongo::count(client, ns, spec.filter, spec.max_time_ms), move |app, res| {
            let Some(tab) = app.tab_mut(tab_id) else { return };
            let p = &mut tab.docs.paged;
            if p.count_seq != seq {
                return;
            }
            p.counting = false;
            match res {
                Ok(n) => {
                    p.total = Some(n);
                    let last = p.last_page().unwrap_or(0);
                    let jump = std::mem::take(&mut p.want_last);
                    if jump {
                        p.pending_select = Some(PendingSelect::Last);
                    }
                    if p.page > last || (jump && p.page != last) {
                        p.page = last;
                        app.load_docs_page(tab_id);
                    } else if jump
                        && let Some(n) = p.result.as_ref().map(|r| r.grid.docs.len()).filter(|n| *n > 0) {
                            p.pending_select = None;
                            p.grid.selected = Some(n - 1);
                            p.grid.scroll_to = Some(n - 1);
                        }
                }
                Err(e) => p.count_error = Some(e),
            }
        });
    }

    fn load_docs_page(&mut self, tab_id: u64) {
        let Some(tab) = self.tab_mut(tab_id) else { return };
        let Some(mut spec) = tab.docs.applied.clone() else { return };
        let p = &mut tab.docs.paged;
        spec.skip = p.page * p.page_size;
        spec.limit = p.page_size;
        p.page_seq += 1;
        p.loading = true;
        let seq = p.page_seq;
        let total = p.total;
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        self.backend.spawn(mongo::find_page(client, ns, spec, total), move |app, res| {
            let Some(tab) = app.tab_mut(tab_id) else { return };
            let p = &mut tab.docs.paged;
            if p.page_seq != seq {
                return;
            }
            p.loading = false;
            apply_page_result(p, res);
        });
    }

    fn explain_find(&mut self, tab_id: u64) {
        let parsed = self.parse_find_spec(tab_id);
        let Some(tab) = self.tab(tab_id) else { return };
        let (spec, _) = match parsed {
            Ok(v) => v,
            Err(e) => {
                self.toast_err(e);
                return;
            }
        };
        let cmd = mongo::explain_find_cmd(&tab.ns, &spec);
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        let title = format!("Explain · {}", tab.ns);
        self.backend.spawn(mongo::explain(client, ns, cmd), move |app, res| match res {
            Ok(d) => app.open_dialog(Dialog::explain(title, &d)),
            Err(e) => app.toast_err(format!("Explain: {e}")),
        });
    }

    pub fn run_agg(&mut self, tab_id: u64) {
        let Some(tab) = self.tab_mut(tab_id) else { return };
        let pipeline = match shell::parse_pipeline(&tab.agg.pipeline) {
            Ok(p) => p,
            Err(e) => {
                tab.agg.paged.error = Some(format!("Pipeline: {e}"));
                return;
            }
        };
        tab.agg.writes = mongo::pipeline_writes(&pipeline);
        tab.agg.applied = Some(pipeline);
        tab.agg.paged.page = 0;
        tab.agg.paged.total = None;
        tab.agg.paged.error = None;
        tab.agg.paged.grid.selected = None;
        let writes = tab.agg.writes;
        if !writes {
            self.start_agg_count(tab_id);
        }
        self.load_agg_page(tab_id);
    }

    fn start_agg_count(&mut self, tab_id: u64) {
        let max_time = self.config.settings.max_time_ms;
        let Some(tab) = self.tab_mut(tab_id) else { return };
        let Some(pipeline) = tab.agg.applied.clone() else { return };
        if tab.agg.writes {
            return;
        }
        let p = &mut tab.agg.paged;
        p.count_seq += 1;
        p.counting = true;
        p.count_error = None;
        let seq = p.count_seq;
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        self.backend.spawn(mongo::aggregate_count(client, ns, pipeline, max_time), move |app, res| {
            let Some(tab) = app.tab_mut(tab_id) else { return };
            let p = &mut tab.agg.paged;
            if p.count_seq != seq {
                return;
            }
            p.counting = false;
            match res {
                Ok(n) => {
                    p.total = Some(n);
                    if std::mem::take(&mut p.want_last) {
                        p.pending_select = Some(PendingSelect::Last);
                        p.page = p.last_page().unwrap_or(0);
                        app.load_agg_page(tab_id);
                    }
                }
                Err(e) => p.count_error = Some(e),
            }
        });
    }

    fn load_agg_page(&mut self, tab_id: u64) {
        let max_time = self.config.settings.max_time_ms;
        let Some(tab) = self.tab_mut(tab_id) else { return };
        let Some(pipeline) = tab.agg.applied.clone() else { return };
        let writes = tab.agg.writes;
        let p = &mut tab.agg.paged;
        p.page_seq += 1;
        p.loading = true;
        let seq = p.page_seq;
        let (skip, limit) = (p.page * p.page_size, p.page_size);
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        self.backend.spawn(mongo::aggregate_page(client, ns, pipeline, skip, limit, max_time), move |app, res| {
            let Some(tab) = app.tab_mut(tab_id) else { return };
            let p = &mut tab.agg.paged;
            if p.page_seq != seq {
                return;
            }
            p.loading = false;
            let ok = res.is_ok();
            apply_page_result(p, res);
            if writes && ok {
                app.toast_ok("Pipeline executado: resultado gravado pelo $out/$merge.");
            }
        });
    }

    fn explain_agg(&mut self, tab_id: u64) {
        let Some(tab) = self.tab(tab_id) else { return };
        let pipeline = match shell::parse_pipeline(&tab.agg.pipeline) {
            Ok(p) => p,
            Err(e) => {
                self.toast_err(format!("Pipeline: {e}"));
                return;
            }
        };
        let cmd = mongo::explain_agg_cmd(&tab.ns, &pipeline);
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        let title = format!("Explain agregação · {}", tab.ns);
        self.backend.spawn(mongo::explain(client, ns, cmd), move |app, res| match res {
            Ok(d) => app.open_dialog(Dialog::explain(title, &d)),
            Err(e) => app.toast_err(format!("Explain: {e}")),
        });
    }

    pub fn load_indexes(&mut self, tab_id: u64) {
        let Some(tab) = self.tab_mut(tab_id) else { return };
        tab.idx.loading = true;
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        self.backend.spawn(mongo::list_indexes(client, ns), move |app, res| {
            let Some(tab) = app.tab_mut(tab_id) else { return };
            tab.idx.loading = false;
            match res {
                Ok(l) => {
                    tab.idx.list = Some(l);
                    tab.idx.error = None;
                }
                Err(e) => tab.idx.error = Some(e),
            }
        });
    }

    fn import_file(&mut self, tab_id: u64, csv: bool) {
        let Some(tab) = self.tab(tab_id) else { return };
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        self.backend.spawn_blocking(
            move || {
                let dialog = rfd::FileDialog::new().set_title("Importar documentos");
                let dialog = if csv {
                    dialog.add_filter("CSV", &["csv"])
                } else {
                    dialog.add_filter("JSON", &["json", "jsonl", "ndjson"])
                };
                dialog.add_filter("Todos", &["*"]).pick_file()
            },
            move |app, path| {
                let Some(path) = path else { return };
                app.toast_ok(format!("Importando {}…", path.display()));
                app.backend.spawn(mongo::import(client, ns, path, csv), move |app, res| match res {
                    Ok(n) => {
                        app.toast_ok(format!("{} documento(s) importado(s).", fmt_num(n)));
                        app.reload_docs(tab_id, true);
                        app.sample_fields(tab_id);
                        app.load_stats(tab_id);
                    }
                    Err(e) => app.toast_err(format!("Importação: {e}")),
                });
            },
        );
    }
}

fn apply_page_result(p: &mut Paged, res: mongo::Res<mongo::Page>) {
    match res {
        Ok(page) => {
            let n = page.docs.len();
            p.result = Some(ResultView {
                grid: GridData::build(page.docs),
                elapsed_ms: page.elapsed_ms,
            });
            p.error = None;
            match p.pending_select.take() {
                Some(PendingSelect::First) if n > 0 => {
                    p.grid.selected = Some(0);
                    p.grid.scroll_to = Some(0);
                }
                Some(PendingSelect::Last) if n > 0 => {
                    p.grid.selected = Some(n - 1);
                    p.grid.scroll_to = Some(n - 1);
                }
                _ => {
                    p.grid.selected = p.grid.selected.filter(|s| *s < n);
                    p.grid.scroll_to = Some(0);
                }
            }
        }
        Err(e) => {
            p.error = Some(e);
            p.pending_select = None;
        }
    }
}

/// Acrescenta um estágio ao fim do texto do pipeline (mantendo o formato de array).
pub fn append_stage(pipeline: &str, stage: &str) -> String {
    let trimmed = pipeline.trim_end();
    if trimmed.is_empty() {
        return format!("[\n  {stage}\n]");
    }
    if let Some(body) = trimmed.strip_suffix(']') {
        let body = body.trim_end();
        let sep = if body.ends_with('[') || body.ends_with(',') { "" } else { "," };
        return format!("{body}{sep}\n  {stage}\n]");
    }
    format!("[\n  {trimmed},\n  {stage}\n]")
}
