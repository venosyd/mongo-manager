//! Janelas de diálogo: conexão, edição de documentos e campos, confirmações,
//! operações em massa, índices, coleções, exportação, comandos e explain.

use crate::app::{App, fmt_num};
use crate::completion::EditorKind;
use crate::config::SavedConnection;
use crate::editor::{CodeEditor, code_view};
use crate::mongo::{self, ExportFormat, Ns};
use crate::shell;
use bson::{Bson, Document, doc};
use egui::text::{CCursor, CCursorRange};
use egui::{Id, Key, Modifiers, RichText, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportSource {
    Query,
    Aggregation,
}

#[derive(Clone, Debug)]
pub enum ConfirmAction {
    RemoveConnection(String),
    DropDatabase { conn: String, db: String },
    DropCollection { conn: String, db: String, coll: String },
    DropIndex { tab: u64, name: String },
    DeleteDocument { tab: u64, id: Bson },
}

#[derive(Clone, Debug)]
pub enum DocMode {
    Edit { id: Bson },
    Insert,
    View,
}

pub struct ConnForm {
    pub id: Option<String>,
    pub name: String,
    pub uri: String,
    pub testing: bool,
    pub test: Option<Result<String, String>>,
}

pub enum Dialog {
    Connection(ConnForm),
    Document {
        tab: u64,
        mode: DocMode,
        text: String,
        error: Option<String>,
        busy: bool,
    },
    Field {
        tab: u64,
        id: Bson,
        field: String,
        text: String,
        type_name: String,
        raw_string: bool,
        error: Option<String>,
        busy: bool,
    },
    Confirm {
        title: String,
        message: String,
        button: String,
        action: ConfirmAction,
        busy: bool,
        error: Option<String>,
    },
    UpdateMany {
        tab: u64,
        filter: String,
        update: String,
        info: Option<String>,
        error: Option<String>,
        busy: bool,
    },
    DeleteMany {
        tab: u64,
        filter: String,
        count: Option<Result<u64, String>>,
        confirmed: bool,
        error: Option<String>,
        busy: bool,
    },
    CreateIndex {
        tab: u64,
        keys: String,
        name: String,
        unique: bool,
        sparse: bool,
        ttl: String,
        partial: String,
        error: Option<String>,
        busy: bool,
    },
    CreateCollection {
        conn: String,
        db: String,
        db_fixed: bool,
        name: String,
        options: String,
        error: Option<String>,
        busy: bool,
    },
    Rename {
        conn: String,
        db: String,
        coll: String,
        new_name: String,
        error: Option<String>,
        busy: bool,
    },
    Export {
        tab: u64,
        source: ExportSource,
        format: ExportFormat,
        all: bool,
        error: Option<String>,
        busy: bool,
    },
    Command {
        conn: String,
        db: String,
        text: String,
        result: Option<String>,
        error: Option<String>,
        busy: bool,
    },
    Explain {
        title: String,
        summary: Vec<(String, String)>,
        warning: Option<String>,
        raw: String,
    },
    Help,
}

impl Dialog {
    pub fn new_connection() -> Self {
        Dialog::Connection(ConnForm {
            id: None,
            name: String::new(),
            uri: "mongodb://localhost:27017".into(),
            testing: false,
            test: None,
        })
    }

    pub fn edit_connection(c: &SavedConnection) -> Self {
        Dialog::Connection(ConnForm {
            id: Some(c.id.clone()),
            name: c.name.clone(),
            uri: c.uri.clone(),
            testing: false,
            test: None,
        })
    }

    pub fn edit_document(tab: u64, d: &Document, readonly: bool) -> Self {
        let mode = match (readonly, d.get("_id")) {
            (false, Some(id)) => DocMode::Edit { id: id.clone() },
            _ => DocMode::View,
        };
        Dialog::Document {
            tab,
            mode,
            text: shell::doc_to_shell_pretty(d),
            error: None,
            busy: false,
        }
    }

    pub fn insert(tab: u64, text: String) -> Self {
        Dialog::Document {
            tab,
            mode: DocMode::Insert,
            text,
            error: None,
            busy: false,
        }
    }

    pub fn edit_field(tab: u64, id: Bson, field: String, value: Option<Bson>) -> Self {
        let (text, type_name, raw_string) = match &value {
            Some(Bson::String(s)) => (s.clone(), "String".to_string(), true),
            Some(v) => (shell::to_shell_pretty(v), shell::type_name(v).to_string(), false),
            None => (String::new(), "(campo ausente)".to_string(), false),
        };
        Dialog::Field {
            tab,
            id,
            field,
            text,
            type_name,
            raw_string,
            error: None,
            busy: false,
        }
    }

    pub fn confirm(title: &str, message: String, button: &str, action: ConfirmAction) -> Self {
        Dialog::Confirm {
            title: title.into(),
            message,
            button: button.into(),
            action,
            busy: false,
            error: None,
        }
    }

    pub fn update_many(tab: u64, filter: String) -> Self {
        Dialog::UpdateMany {
            tab,
            filter,
            update: "{ $set: {  } }".into(),
            info: None,
            error: None,
            busy: false,
        }
    }

    pub fn delete_many(tab: u64, filter: String) -> Self {
        Dialog::DeleteMany {
            tab,
            filter,
            count: None,
            confirmed: false,
            error: None,
            busy: false,
        }
    }

    pub fn create_index(tab: u64) -> Self {
        Dialog::CreateIndex {
            tab,
            keys: "{  }".into(),
            name: String::new(),
            unique: false,
            sparse: false,
            ttl: String::new(),
            partial: String::new(),
            error: None,
            busy: false,
        }
    }

    pub fn create_collection(conn: String, db: Option<String>) -> Self {
        Dialog::CreateCollection {
            conn,
            db_fixed: db.is_some(),
            db: db.unwrap_or_default(),
            name: String::new(),
            options: String::new(),
            error: None,
            busy: false,
        }
    }

    pub fn rename(conn: String, db: String, coll: String) -> Self {
        Dialog::Rename {
            conn,
            db,
            new_name: coll.clone(),
            coll,
            error: None,
            busy: false,
        }
    }

    pub fn export(tab: u64, source: ExportSource) -> Self {
        Dialog::Export {
            tab,
            source,
            format: ExportFormat::JsonArray,
            all: true,
            error: None,
            busy: false,
        }
    }

    pub fn command(conn: String, db: String) -> Self {
        Dialog::Command {
            conn,
            db,
            text: "{ ping: 1 }".into(),
            result: None,
            error: None,
            busy: false,
        }
    }

    pub fn explain(title: String, d: &Document) -> Self {
        let (summary, warning) = explain_summary(d);
        Dialog::Explain {
            title,
            summary,
            warning,
            raw: shell::doc_to_shell_pretty(d),
        }
    }
}

fn find_key<'a>(d: &'a Document, key: &str, depth: usize) -> Option<&'a Bson> {
    if let Some(v) = d.get(key) {
        return Some(v);
    }
    if depth > 12 {
        return None;
    }
    for v in d.values() {
        match v {
            Bson::Document(sub) => {
                if let Some(found) = find_key(sub, key, depth + 1) {
                    return Some(found);
                }
            }
            Bson::Array(a) => {
                for item in a {
                    if let Bson::Document(sub) = item
                        && let Some(found) = find_key(sub, key, depth + 1) {
                            return Some(found);
                        }
                }
            }
            _ => {}
        }
    }
    None
}

fn plan_chain(plan: &Document, out: &mut Vec<String>, indexes: &mut Vec<String>) {
    let stage = plan.get_str("stage").unwrap_or("?");
    match plan.get_str("indexName") {
        Ok(ix) => {
            out.push(format!("{stage} ({ix})"));
            indexes.push(ix.to_string());
        }
        Err(_) => out.push(stage.to_string()),
    }
    if let Ok(input) = plan.get_document("inputStage") {
        plan_chain(input, out, indexes);
    }
    if let Ok(inputs) = plan.get_array("inputStages") {
        for i in inputs.iter().filter_map(|b| b.as_document()) {
            plan_chain(i, out, indexes);
        }
    }
    if let Ok(qp) = plan.get_document("queryPlan") {
        plan_chain(qp, out, indexes);
    }
}

fn explain_summary(d: &Document) -> (Vec<(String, String)>, Option<String>) {
    let mut out = Vec::new();
    let mut warning = None;
    if let Some(Bson::Document(wp)) = find_key(d, "winningPlan", 0) {
        let mut chain = Vec::new();
        let mut indexes = Vec::new();
        plan_chain(wp, &mut chain, &mut indexes);
        out.push(("Plano vencedor".into(), chain.join(" ⬅ ")));
        if indexes.is_empty() {
            out.push(("Índice usado".into(), "nenhum".into()));
        } else {
            out.push(("Índice usado".into(), indexes.join(", ")));
        }
        if chain.iter().any(|s| s.starts_with("COLLSCAN")) {
            warning = Some("Varredura completa da coleção (COLLSCAN): considere criar um índice para este filtro/ordenação.".into());
        }
        if chain.iter().any(|s| s.starts_with("SORT")) && !chain.iter().any(|s| s.starts_with("SORT_KEY")) {
            out.push(("Ordenação".into(), "em memória (sem índice)".into()));
        }
    }
    if let Some(Bson::Document(es)) = find_key(d, "executionStats", 0) {
        for (k, label) in [
            ("nReturned", "Documentos retornados"),
            ("totalDocsExamined", "Documentos examinados"),
            ("totalKeysExamined", "Chaves de índice examinadas"),
            ("executionTimeMillis", "Tempo de execução (ms)"),
        ] {
            if let Some(v) = es.get(k) {
                out.push((label.into(), shell::to_shell_compact(v).replace("NumberLong(", "").replace(')', "")));
            }
        }
    }
    if out.is_empty() {
        out.push(("Resumo".into(), "não disponível — veja o resultado completo abaixo".into()));
    }
    (out, warning)
}

fn error_label(ui: &mut Ui, error: &Option<String>) {
    if let Some(e) = error {
        ui.add(egui::Label::new(RichText::new(format!("⚠ {e}")).color(ui.visuals().error_fg_color)).wrap());
    }
}

fn index_name(keys: &Document) -> String {
    keys.iter()
        .map(|(k, v)| {
            let v = match v {
                Bson::String(s) => s.clone(),
                Bson::Int32(n) => n.to_string(),
                Bson::Int64(n) => n.to_string(),
                Bson::Double(n) => n.to_string(),
                other => shell::to_shell_compact(other),
            };
            format!("{k}_{v}")
        })
        .collect::<Vec<_>>()
        .join("_")
}

/// Campo que recebe o foco quando o diálogo abre, e a seleção inicial do texto.
fn primary_field(uid: u64, d: &Dialog) -> Option<(Id, CCursorRange)> {
    let all = |t: &str| CCursorRange::two(CCursor::new(0), CCursor::new(t.chars().count()));
    let at = |n: usize| CCursorRange::one(CCursor::new(n));
    match d {
        Dialog::Connection(f) => Some((Id::unique(("conn_name", uid)), all(&f.name))),
        Dialog::Document { mode: DocMode::View, .. } => None,
        Dialog::Document { mode: DocMode::Insert, text, .. } => {
            let pos = if text.trim() == "{\n  \n}".trim() { 4 } else { 0 };
            Some((Id::unique(("doc_editor", uid)), at(pos)))
        }
        Dialog::Document { .. } => Some((Id::unique(("doc_editor", uid)), at(0))),
        Dialog::Field { raw_string: true, text, .. } => Some((Id::unique(("field_raw", uid)), all(text))),
        Dialog::Field { text, .. } => Some((Id::unique(("field_editor", uid)), all(text))),
        Dialog::UpdateMany { update, .. } => Some((Id::unique(("um_update", uid)), at(update.find("{  }").map_or(0, |p| p + 2)))),
        Dialog::DeleteMany { filter, .. } => Some((Id::unique(("dm_filter", uid)), at(filter.chars().count()))),
        Dialog::CreateIndex { .. } => Some((Id::unique(("ix_keys", uid)), at(2))),
        Dialog::CreateCollection { db_fixed: false, .. } => Some((Id::unique(("cc_db", uid)), at(0))),
        Dialog::CreateCollection { .. } => Some((Id::unique(("cc_name", uid)), at(0))),
        Dialog::Rename { new_name, .. } => Some((Id::unique(("rename", uid)), all(new_name))),
        Dialog::Command { text, .. } => Some((Id::unique(("cmd", uid)), all(text))),
        _ => None,
    }
}

enum Outcome {
    Keep,
    Close,
}

impl App {
    pub fn show_dialogs(&mut self, ctx: &egui::Context) {
        let dialogs = std::mem::take(&mut self.dialogs);
        let count = dialogs.len();
        let mut keep = Vec::new();
        for (i, (uid, mut d)) in dialogs.into_iter().enumerate() {
            let topmost = i + 1 == count;
            let mut open = true;
            let outcome = self.show_dialog(ctx, uid, &mut d, &mut open);
            let escape = topmost && ctx.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::Escape));
            let busy = matches!(
                &d,
                Dialog::Document { busy: true, .. }
                    | Dialog::Field { busy: true, .. }
                    | Dialog::Confirm { busy: true, .. }
                    | Dialog::UpdateMany { busy: true, .. }
                    | Dialog::DeleteMany { busy: true, .. }
            );
            if open && !(escape && !busy) && matches!(outcome, Outcome::Keep) {
                keep.push((uid, d));
            }
        }
        keep.extend(std::mem::take(&mut self.dialogs));
        self.dialogs = keep;
    }

    fn show_dialog(&mut self, ctx: &egui::Context, uid: u64, d: &mut Dialog, open: &mut bool) -> Outcome {
        let mut outcome = Outcome::Keep;
        // A janela usa o primeiro quadro para medir o conteúdo; o foco vai no segundo.
        let frames = self.autofocused.entry(uid).or_insert(0);
        *frames = frames.saturating_add(1);
        if *frames == 2
            && let Some((id, range)) = primary_field(uid, d) {
                ctx.memory_mut(|m| m.request_focus(id));
                let mut st = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
                st.cursor.set_char_range(Some(range));
                st.store(ctx, id);
            }
        let screen = ctx.content_rect();
        let win = |title: &str| {
            egui::Window::new(title.to_string())
                .id(Id::unique(("dialog", uid)))
                .collapsible(false)
                .resizable(true)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(screen.center())
                .max_height(screen.height() - 40.0)
        };
        match d {
            Dialog::Connection(f) => {
                let title = if f.id.is_some() { "Editar conexão" } else { "Nova conexão" };
                win(title).default_width(560.0).open(open).show(ctx, |ui| {
                    egui::Grid::new(("conn_form", uid)).num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        ui.label("Nome");
                        ui.add(egui::TextEdit::singleline(&mut f.name).id(Id::unique(("conn_name", uid))).hint_text("ex.: produção").desired_width(420.0));
                        ui.end_row();
                        ui.label("URI");
                        ui.add(
                            egui::TextEdit::singleline(&mut f.uri)
                                .hint_text("mongodb://usuario:senha@host:27017/?authSource=admin")
                                .desired_width(420.0)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.end_row();
                    });
                    ui.label(
                        RichText::new("Aceita mongodb:// e mongodb+srv://, com opções como authSource, replicaSet, tls, directConnection…")
                            .small()
                            .weak(),
                    );
                    ui.add_space(6.0);
                    match &f.test {
                        Some(Ok(m)) => {
                            ui.colored_label(crate::app::ACCENT_DARK.linear_multiply(1.6), format!("✔ {m}"));
                        }
                        Some(Err(e)) => error_label(ui, &Some(e.clone())),
                        None => {}
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!f.testing, egui::Button::new("Testar conexão")).clicked() {
                            f.testing = true;
                            f.test = None;
                            let uri = f.uri.trim().to_string();
                            self.backend.spawn(
                                async move {
                                    let r = mongo::connect(uri).await;
                                    match r {
                                        Ok(c) => {
                                            let msg = format!("Conectado: MongoDB {}, {} banco(s) visíveis", c.version, c.dbs.len());
                                            c.client.shutdown().await;
                                            Ok(msg)
                                        }
                                        Err(e) => Err(e),
                                    }
                                },
                                move |app, res| {
                                    if let Some(Dialog::Connection(f)) = app.dialog_mut(uid) {
                                        f.testing = false;
                                        f.test = Some(res);
                                    }
                                },
                            );
                        }
                        if f.testing {
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let valid = f.uri.trim().starts_with("mongodb");
                            let save_connect = ui.add_enabled(valid, egui::Button::new(RichText::new("Salvar e conectar").strong())).clicked();
                            let save = ui.add_enabled(valid, egui::Button::new("Salvar")).clicked();
                            if ui.button("Cancelar").clicked() {
                                outcome = Outcome::Close;
                            }
                            if save || save_connect {
                                let id = f.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                                let name = if f.name.trim().is_empty() {
                                    f.uri
                                        .split("://")
                                        .nth(1)
                                        .map(|s| s.rsplit('@').next().unwrap_or(s).split(['/', '?']).next().unwrap_or(s).to_string())
                                        .unwrap_or_else(|| "conexão".into())
                                } else {
                                    f.name.trim().to_string()
                                };
                                let uri = f.uri.trim().to_string();
                                let mut uri_changed = false;
                                match self.config.connections.iter_mut().find(|c| c.id == id) {
                                    Some(c) => {
                                        uri_changed = c.uri != uri;
                                        c.name = name;
                                        c.uri = uri;
                                    }
                                    None => self.config.connections.push(SavedConnection { id: id.clone(), name, uri }),
                                }
                                self.save_config();
                                if uri_changed {
                                    self.disconnect(&id);
                                }
                                if save_connect {
                                    self.connect(&id);
                                }
                                outcome = Outcome::Close;
                            }
                        });
                    });
                });
            }

            Dialog::Document { tab, mode, text, error, busy } => {
                let tab_id = *tab;
                let fields = self.tab(tab_id).map(|t| t.fields.clone()).unwrap_or_default();
                let ns = self.tab(tab_id).map(|t| t.ns.to_string()).unwrap_or_default();
                let title = match mode {
                    DocMode::Edit { .. } => format!("Editar documento · {ns}"),
                    DocMode::Insert => format!("Inserir documento(s) · {ns}"),
                    DocMode::View => format!("Documento · {ns}"),
                };
                let readonly = matches!(mode, DocMode::View);
                win(&title)
                    .default_width((screen.width() * 0.55).max(520.0))
                    .default_height(screen.height() * 0.7)
                    .open(open)
                    .show(ctx, |ui| {
                        if matches!(mode, DocMode::Insert) {
                            ui.label(
                                RichText::new("Um documento { … }, um array [ {…}, {…} ] ou um documento por linha. _id é gerado se omitido.")
                                    .small()
                                    .weak(),
                            );
                        }
                        let mut submit = false;
                        let h = ui.available_height() - 70.0;
                        egui::ScrollArea::vertical().max_height(h.max(150.0)).auto_shrink([false, true]).show(ui, |ui| {
                            if readonly {
                                code_view(ui, text);
                            } else {
                                let out = CodeEditor::new(Id::unique(("doc_editor", uid)), text, EditorKind::Document, &fields)
                                    .rows(18)
                                    .show(ui);
                                submit = out.submit;
                            }
                        });
                        error_label(ui, error);
                        ui.separator();
                        ui.horizontal(|ui| {
                            if !readonly && ui.button("Formatar").on_hover_text("Reindentar o texto").clicked() {
                                match shell::parse_documents(text) {
                                    Ok(docs) if docs.len() == 1 => *text = shell::doc_to_shell_pretty(&docs[0]),
                                    Ok(docs) => *text = shell::docs_to_shell_pretty(&docs),
                                    Err(e) => *error = Some(e.to_string()),
                                }
                            }
                            if ui.button("Copiar").clicked() {
                                ui.ctx().copy_text(text.clone());
                            }
                            if *busy {
                                ui.spinner();
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if readonly {
                                    if ui.button("Fechar").clicked() {
                                        outcome = Outcome::Close;
                                    }
                                    return;
                                }
                                let label = if matches!(mode, DocMode::Insert) { "Inserir" } else { "Salvar" };
                                let save = ui
                                    .add_enabled(!*busy, egui::Button::new(RichText::new(label).strong()))
                                    .on_hover_text("Ctrl+Enter")
                                    .clicked();
                                if ui.button("Cancelar").clicked() {
                                    outcome = Outcome::Close;
                                }
                                if (save || submit) && !*busy {
                                    *error = None;
                                    self.save_document(uid, tab_id, mode.clone(), text, error, busy);
                                }
                            });
                        });
                    });
            }

            Dialog::Field { tab, id, field, text, type_name, raw_string, error, busy } => {
                let tab_id = *tab;
                let fields = self.tab(tab_id).map(|t| t.fields.clone()).unwrap_or_default();
                win(&format!("Editar campo \"{field}\"")).default_width(520.0).open(open).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Tipo atual: {type_name}")).weak());
                        ui.label(RichText::new(format!("_id: {}", shell::to_shell_compact(id))).weak().small());
                    });
                    ui.checkbox(raw_string, "Texto puro (salvar como String, sem aspas)");
                    let multi = text.contains('\n') || text.len() > 60;
                    let out = egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                        if *raw_string {
                            let r = ui.add(
                                egui::TextEdit::multiline(text)
                                    .id(Id::unique(("field_raw", uid)))
                                    .desired_rows(if multi { 8 } else { 2 })
                                    .desired_width(f32::INFINITY),
                            );
                            
                            r.has_focus() && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter))
                        } else {
                            CodeEditor::new(Id::unique(("field_editor", uid)), text, EditorKind::Document, &fields)
                                .rows(if multi { 10 } else { 2 })
                                .hint("123   \"texto\"   true   ISODate(\"2024-01-01\")   ObjectId(\"…\")   { a: 1 }   [1, 2]")
                                .show(ui)
                                .submit
                        }
                    });
                    if !*raw_string {
                        ui.label(RichText::new("Sintaxe do shell: números, \"strings\", ISODate(…), ObjectId(…), NumberLong(…), objetos e arrays.").small().weak());
                    }
                    error_label(ui, error);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if *busy {
                            ui.spinner();
                        }
                        let unset = ui
                            .add_enabled(!*busy, egui::Button::new(RichText::new("Remover campo").color(ui.visuals().error_fg_color)))
                            .on_hover_text("$unset")
                            .clicked();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let save = ui.add_enabled(!*busy, egui::Button::new(RichText::new("Salvar").strong())).on_hover_text("Ctrl+Enter").clicked();
                            if ui.button("Cancelar").clicked() {
                                outcome = Outcome::Close;
                            }
                            let update = if unset {
                                Some(doc! { "$unset": { field.clone(): "" } })
                            } else if save || out.inner {
                                let value = if *raw_string {
                                    Ok(Bson::String(text.clone()))
                                } else {
                                    shell::parse_value(text)
                                };
                                match value {
                                    Ok(v) => Some(doc! { "$set": { field.clone(): v } }),
                                    Err(e) => {
                                        *error = Some(e.to_string());
                                        None
                                    }
                                }
                            } else {
                                None
                            };
                            if let (Some(update), Some(tab)) = (update, self.tab(tab_id)) {
                                *busy = true;
                                *error = None;
                                let (client, ns) = (tab.client.clone(), tab.ns.clone());
                                self.backend.spawn(mongo::update_one_by_id(client, ns, id.clone(), update), move |app, res| match res {
                                    Ok(_) => {
                                        app.close_dialog(uid);
                                        app.toast_ok("Campo atualizado.");
                                        app.reload_docs(tab_id, false);
                                    }
                                    Err(e) => {
                                        if let Some(Dialog::Field { error, busy, .. }) = app.dialog_mut(uid) {
                                            *error = Some(e);
                                            *busy = false;
                                        }
                                    }
                                });
                            }
                        });
                    });
                });
            }

            Dialog::Confirm { title, message, button, action, busy, error } => {
                win(title).default_width(440.0).resizable(false).open(open).show(ctx, |ui| {
                    ui.add(egui::Label::new(message.as_str()).wrap());
                    if matches!(action, ConfirmAction::DeleteDocument { .. } | ConfirmAction::DropIndex { .. }) {
                        ui.label(RichText::new("Enter confirma · Esc cancela").small().weak());
                    }
                    error_label(ui, error);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if *busy {
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let go = ui
                                .add_enabled(
                                    !*busy,
                                    egui::Button::new(RichText::new(button.as_str()).strong().color(egui::Color32::WHITE))
                                        .fill(egui::Color32::from_rgb(0xC6, 0x28, 0x28)),
                                )
                                .clicked();
                            if ui.button("Cancelar").clicked() {
                                outcome = Outcome::Close;
                            }
                            // Enter confirma ações de baixo impacto (excluir um documento ou índice).
                            let enter_ok = matches!(action, ConfirmAction::DeleteDocument { .. } | ConfirmAction::DropIndex { .. })
                                && self.autofocused.get(&uid).is_some_and(|f| *f > 2)
                                && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                            if (go || enter_ok) && !*busy {
                                *busy = true;
                                if self.run_confirm(uid, action.clone()) {
                                    outcome = Outcome::Close;
                                }
                            }
                        });
                    });
                });
            }

            Dialog::UpdateMany { tab, filter, update, info, error, busy } => {
                let tab_id = *tab;
                let fields = self.tab(tab_id).map(|t| t.fields.clone()).unwrap_or_default();
                win("Atualizar vários documentos (updateMany)").default_width(640.0).open(open).show(ctx, |ui| {
                    ui.label(RichText::new("Filtro").strong());
                    CodeEditor::new(Id::unique(("um_filter", uid)), filter, EditorKind::Filter, &fields)
                        .rows(2)
                        .hint("{ }  (vazio = todos os documentos)")
                        .show(ui);
                    ui.label(RichText::new("Atualização").strong());
                    CodeEditor::new(Id::unique(("um_update", uid)), update, EditorKind::Update, &fields)
                        .rows(6)
                        .hint("{ $set: { campo: valor } }  ou pipeline [ { $set: … } ]")
                        .show(ui);
                    if let Some(i) = info {
                        ui.label(RichText::new(i.as_str()).weak());
                    }
                    error_label(ui, error);
                    ui.separator();
                    ui.horizontal(|ui| {
                        let preview = ui.button("Contar afetados").clicked();
                        if *busy {
                            ui.spinner();
                        }
                        let parsed = shell::parse_document(filter).map_err(|e| format!("Filtro: {e}"));
                        if preview {
                            match (&parsed, self.tab(tab_id)) {
                                (Ok(f), Some(t)) => {
                                    let (client, ns) = (t.client.clone(), t.ns.clone());
                                    self.backend.spawn(mongo::count(client, ns, f.clone(), 60_000), move |app, res| {
                                        if let Some(Dialog::UpdateMany { info, error, .. }) = app.dialog_mut(uid) {
                                            match res {
                                                Ok(n) => *info = Some(format!("{} documento(s) correspondem ao filtro.", fmt_num(n))),
                                                Err(e) => *error = Some(e),
                                            }
                                        }
                                    });
                                }
                                (Err(e), _) => *error = Some(e.clone()),
                                _ => {}
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let go = ui.add_enabled(!*busy, egui::Button::new(RichText::new("Executar").strong())).clicked();
                            if ui.button("Cancelar").clicked() {
                                outcome = Outcome::Close;
                            }
                            if go {
                                let upd = shell::parse_value(update).map_err(|e| format!("Atualização: {e}"));
                                match (parsed, upd, self.tab(tab_id)) {
                                    (Ok(f), Ok(u), Some(t)) => {
                                        *busy = true;
                                        *error = None;
                                        let (client, ns) = (t.client.clone(), t.ns.clone());
                                        self.backend.spawn(mongo::update_many(client, ns, f, u), move |app, res| match res {
                                            Ok((matched, modified)) => {
                                                app.close_dialog(uid);
                                                app.toast_ok(format!(
                                                    "{} correspondente(s), {} modificado(s).",
                                                    fmt_num(matched),
                                                    fmt_num(modified)
                                                ));
                                                app.reload_docs(tab_id, false);
                                            }
                                            Err(e) => {
                                                if let Some(Dialog::UpdateMany { error, busy, .. }) = app.dialog_mut(uid) {
                                                    *error = Some(e);
                                                    *busy = false;
                                                }
                                            }
                                        });
                                    }
                                    (Err(e), _, _) | (_, Err(e), _) => *error = Some(e),
                                    _ => {}
                                }
                            }
                        });
                    });
                });
            }

            Dialog::DeleteMany { tab, filter, count, confirmed, error, busy } => {
                let tab_id = *tab;
                let fields = self.tab(tab_id).map(|t| t.fields.clone()).unwrap_or_default();
                win("Excluir vários documentos (deleteMany)").default_width(600.0).open(open).show(ctx, |ui| {
                    ui.label(RichText::new("Filtro").strong());
                    let out = CodeEditor::new(Id::unique(("dm_filter", uid)), filter, EditorKind::Filter, &fields)
                        .rows(2)
                        .hint("{ }  (vazio = TODOS os documentos)")
                        .show(ui);
                    if out.changed {
                        *count = None;
                        *confirmed = false;
                    }
                    let parsed = shell::parse_document(filter).map_err(|e| format!("Filtro: {e}"));
                    if count.is_none() && !*busy
                        && let (Ok(f), Some(t)) = (&parsed, self.tab(tab_id)) {
                            *count = Some(Err("contando…".into()));
                            let (client, ns) = (t.client.clone(), t.ns.clone());
                            self.backend.spawn(mongo::count(client, ns, f.clone(), 60_000), move |app, res| {
                                if let Some(Dialog::DeleteMany { count, .. }) = app.dialog_mut(uid) {
                                    *count = Some(res);
                                }
                            });
                        }
                    match (&parsed, count.as_ref()) {
                        (Err(e), _) => error_label(ui, &Some(e.clone())),
                        (Ok(f), Some(Ok(n))) => {
                            let warn = if f.is_empty() { " — filtro vazio: TODOS os documentos!" } else { "" };
                            ui.label(
                                RichText::new(format!("{} documento(s) serão excluídos{warn}", fmt_num(*n)))
                                    .strong()
                                    .color(ui.visuals().warn_fg_color),
                            );
                        }
                        (_, Some(Err(m))) => {
                            ui.label(RichText::new(m.as_str()).weak());
                        }
                        _ => {}
                    }
                    ui.checkbox(confirmed, "Entendo que esta ação não pode ser desfeita");
                    error_label(ui, error);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if *busy {
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let go = ui
                                .add_enabled(
                                    *confirmed && !*busy && parsed.is_ok(),
                                    egui::Button::new(RichText::new("Excluir").strong().color(egui::Color32::WHITE))
                                        .fill(egui::Color32::from_rgb(0xC6, 0x28, 0x28)),
                                )
                                .clicked();
                            if ui.button("Cancelar").clicked() {
                                outcome = Outcome::Close;
                            }
                            if let (true, Ok(f), Some(t)) = (go, &parsed, self.tab(tab_id)) {
                                *busy = true;
                                let (client, ns) = (t.client.clone(), t.ns.clone());
                                self.backend.spawn(mongo::delete_many(client, ns, f.clone()), move |app, res| match res {
                                    Ok(n) => {
                                        app.close_dialog(uid);
                                        app.toast_ok(format!("{} documento(s) excluído(s).", fmt_num(n)));
                                        app.reload_docs(tab_id, true);
                                        app.load_stats(tab_id);
                                    }
                                    Err(e) => {
                                        if let Some(Dialog::DeleteMany { error, busy, .. }) = app.dialog_mut(uid) {
                                            *error = Some(e);
                                            *busy = false;
                                        }
                                    }
                                });
                            }
                        });
                    });
                });
            }

            Dialog::CreateIndex { tab, keys, name, unique, sparse, ttl, partial, error, busy } => {
                let tab_id = *tab;
                let fields = self.tab(tab_id).map(|t| t.fields.clone()).unwrap_or_default();
                win("Criar índice").default_width(560.0).open(open).show(ctx, |ui| {
                    ui.label(RichText::new("Chaves").strong());
                    CodeEditor::new(Id::unique(("ix_keys", uid)), keys, EditorKind::IndexKeys, &fields)
                        .rows(3)
                        .hint("{ campo: 1, outro: -1 }   1 = crescente · -1 = decrescente · \"text\" · \"2dsphere\" · \"hashed\"")
                        .show(ui);
                    egui::Grid::new(("ix_opts", uid)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Nome");
                        ui.add(egui::TextEdit::singleline(name).hint_text("(automático)").desired_width(300.0));
                        ui.end_row();
                        ui.label("Opções");
                        ui.horizontal(|ui| {
                            ui.checkbox(unique, "Único");
                            ui.checkbox(sparse, "Sparse");
                        });
                        ui.end_row();
                        ui.label("TTL (segundos)");
                        ui.add(egui::TextEdit::singleline(ttl).hint_text("expireAfterSeconds").desired_width(120.0));
                        ui.end_row();
                    });
                    ui.label(RichText::new("Filtro parcial (partialFilterExpression)").strong());
                    CodeEditor::new(Id::unique(("ix_partial", uid)), partial, EditorKind::Filter, &fields)
                        .rows(1)
                        .hint("{ campo: { $exists: true } }")
                        .show(ui);
                    error_label(ui, error);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if *busy {
                            ui.spinner();
                            ui.label("Criando índice…");
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let go = ui.add_enabled(!*busy, egui::Button::new(RichText::new("Criar").strong())).clicked();
                            if ui.button("Cancelar").clicked() {
                                outcome = Outcome::Close;
                            }
                            if !go {
                                return;
                            }
                            let built = (|| -> Result<Document, String> {
                                let k = shell::parse_document(keys).map_err(|e| format!("Chaves: {e}"))?;
                                if k.is_empty() {
                                    return Err("Informe ao menos uma chave.".into());
                                }
                                let n = if name.trim().is_empty() { index_name(&k) } else { name.trim().to_string() };
                                let mut ix = doc! { "key": k, "name": n };
                                if *unique {
                                    ix.insert("unique", true);
                                }
                                if *sparse {
                                    ix.insert("sparse", true);
                                }
                                if !ttl.trim().is_empty() {
                                    let secs: i64 = ttl.trim().parse().map_err(|_| "TTL deve ser um número inteiro de segundos".to_string())?;
                                    ix.insert("expireAfterSeconds", secs);
                                }
                                let p = shell::parse_document(partial).map_err(|e| format!("Filtro parcial: {e}"))?;
                                if !p.is_empty() {
                                    ix.insert("partialFilterExpression", p);
                                }
                                Ok(ix)
                            })();
                            match (built, self.tab(tab_id)) {
                                (Ok(ix), Some(t)) => {
                                    *busy = true;
                                    *error = None;
                                    let (client, ns) = (t.client.clone(), t.ns.clone());
                                    self.backend.spawn(mongo::create_index(client, ns, ix), move |app, res| match res {
                                        Ok(_) => {
                                            app.close_dialog(uid);
                                            app.toast_ok("Índice criado.");
                                            app.load_indexes(tab_id);
                                            app.load_stats(tab_id);
                                        }
                                        Err(e) => {
                                            if let Some(Dialog::CreateIndex { error, busy, .. }) = app.dialog_mut(uid) {
                                                *error = Some(e);
                                                *busy = false;
                                            }
                                        }
                                    });
                                }
                                (Err(e), _) => *error = Some(e),
                                _ => {}
                            }
                        });
                    });
                });
            }

            Dialog::CreateCollection { conn, db, db_fixed, name, options, error, busy } => {
                let title = if *db_fixed { format!("Criar coleção em \"{db}\"") } else { "Criar banco de dados".to_string() };
                win(&title).default_width(520.0).open(open).show(ctx, |ui| {
                    egui::Grid::new(("cc", uid)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        if !*db_fixed {
                            ui.label("Banco");
                            ui.add(egui::TextEdit::singleline(db).id(Id::unique(("cc_db", uid))).desired_width(300.0));
                            ui.end_row();
                        }
                        ui.label("Coleção");
                        ui.add(egui::TextEdit::singleline(name).id(Id::unique(("cc_name", uid))).desired_width(300.0));
                        ui.end_row();
                    });
                    if !*db_fixed {
                        ui.label(RichText::new("O MongoDB cria o banco junto com a primeira coleção.").small().weak());
                    }
                    ui.label(RichText::new("Opções (opcional)").strong());
                    CodeEditor::new(Id::unique(("cc_opts", uid)), options, EditorKind::Command, &[])
                        .rows(2)
                        .hint("{ capped: true, size: 1048576 }   ·   { viewOn: \"origem\", pipeline: [] }")
                        .show(ui);
                    error_label(ui, error);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if *busy {
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let valid = !db.trim().is_empty() && !name.trim().is_empty();
                            let go = ui.add_enabled(valid && !*busy, egui::Button::new(RichText::new("Criar").strong())).clicked();
                            if ui.button("Cancelar").clicked() {
                                outcome = Outcome::Close;
                            }
                            if !go {
                                return;
                            }
                            match (shell::parse_document(options), self.client(conn)) {
                                (Ok(opts), Some(client)) => {
                                    *busy = true;
                                    let (conn, db_name, coll) = (conn.clone(), db.trim().to_string(), name.trim().to_string());
                                    let is_new_db = !*db_fixed;
                                    self.backend.spawn(
                                        mongo::create_collection(client, Ns::new(db_name.clone(), coll.clone()), opts),
                                        move |app, res| match res {
                                            Ok(()) => {
                                                app.close_dialog(uid);
                                                app.toast_ok(format!("Coleção {db_name}.{coll} criada."));
                                                if is_new_db {
                                                    app.refresh_conn(&conn);
                                                } else {
                                                    app.load_collections(&conn, &db_name);
                                                }
                                            }
                                            Err(e) => {
                                                if let Some(Dialog::CreateCollection { error, busy, .. }) = app.dialog_mut(uid) {
                                                    *error = Some(e);
                                                    *busy = false;
                                                }
                                            }
                                        },
                                    );
                                }
                                (Err(e), _) => *error = Some(format!("Opções: {e}")),
                                (_, None) => *error = Some("Conexão não está ativa.".into()),
                            }
                        });
                    });
                });
            }

            Dialog::Rename { conn, db, coll, new_name, error, busy } => {
                win(&format!("Renomear \"{db}.{coll}\"")).default_width(420.0).open(open).show(ctx, |ui| {
                    ui.add(egui::TextEdit::singleline(new_name).id(Id::unique(("rename", uid))).desired_width(f32::INFINITY));
                    error_label(ui, error);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if *busy {
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let valid = !new_name.trim().is_empty() && new_name.trim() != coll;
                            let go = ui.add_enabled(valid && !*busy, egui::Button::new(RichText::new("Renomear").strong())).clicked();
                            if ui.button("Cancelar").clicked() {
                                outcome = Outcome::Close;
                            }
                            if let (true, Some(client)) = (go, self.client(conn)) {
                                *busy = true;
                                let (conn, db, coll, new) = (conn.clone(), db.clone(), coll.clone(), new_name.trim().to_string());
                                self.backend.spawn(
                                    mongo::rename_collection(client, Ns::new(db.clone(), coll.clone()), new.clone()),
                                    move |app, res| match res {
                                        Ok(()) => {
                                            app.close_dialog(uid);
                                            for t in app.tabs.iter_mut().filter(|t| t.conn_id == conn && t.ns.db == db && t.ns.coll == coll) {
                                                t.ns.coll = new.clone();
                                            }
                                            app.toast_ok(format!("Coleção renomeada para {new}."));
                                            app.load_collections(&conn, &db);
                                        }
                                        Err(e) => {
                                            if let Some(Dialog::Rename { error, busy, .. }) = app.dialog_mut(uid) {
                                                *error = Some(e);
                                                *busy = false;
                                            }
                                        }
                                    },
                                );
                            }
                        });
                    });
                });
            }

            Dialog::Export { tab, source, format, all, error, busy } => {
                let tab_id = *tab;
                win("Exportar").default_width(460.0).resizable(false).open(open).show(ctx, |ui| {
                    ui.label(RichText::new("Formato").strong());
                    ui.radio_value(format, ExportFormat::JsonArray, "JSON (array, Extended JSON relaxado)");
                    ui.radio_value(format, ExportFormat::JsonLines, "JSON Lines (um documento por linha)");
                    ui.radio_value(format, ExportFormat::Csv, "CSV (campos aninhados viram colunas a.b)");
                    ui.add_space(6.0);
                    ui.label(RichText::new("Conteúdo").strong());
                    let what = if *source == ExportSource::Query { "consulta" } else { "agregação" };
                    ui.radio_value(all, true, format!("Todo o resultado da {what}"));
                    ui.radio_value(all, false, "Somente a página atual");
                    error_label(ui, error);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if *busy {
                            ui.spinner();
                            ui.label("Exportando…");
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let go = ui.add_enabled(!*busy, egui::Button::new(RichText::new("Escolher arquivo…").strong())).clicked();
                            if ui.button("Cancelar").clicked() {
                                outcome = Outcome::Close;
                            }
                            if go {
                                match self.export_request(tab_id, *source, *all) {
                                    Ok((client, ns, src, default_name)) => {
                                        *busy = true;
                                        let fmt = *format;
                                        let ext = match fmt {
                                            ExportFormat::JsonArray => "json",
                                            ExportFormat::JsonLines => "jsonl",
                                            ExportFormat::Csv => "csv",
                                        };
                                        let file_name = format!("{default_name}.{ext}");
                                        self.backend.spawn_blocking(
                                            move || rfd::FileDialog::new().set_title("Exportar").set_file_name(&file_name).save_file(),
                                            move |app, path| {
                                                let Some(path) = path else {
                                                    if let Some(Dialog::Export { busy, .. }) = app.dialog_mut(uid) {
                                                        *busy = false;
                                                    }
                                                    return;
                                                };
                                                let shown = path.display().to_string();
                                                app.backend.spawn(mongo::export(client, ns, src, path, fmt), move |app, res| match res {
                                                    Ok(n) => {
                                                        app.close_dialog(uid);
                                                        app.toast_ok(format!("{} documento(s) exportado(s) para {shown}", fmt_num(n)));
                                                    }
                                                    Err(e) => {
                                                        if let Some(Dialog::Export { error, busy, .. }) = app.dialog_mut(uid) {
                                                            *error = Some(e);
                                                            *busy = false;
                                                        }
                                                    }
                                                });
                                            },
                                        );
                                    }
                                    Err(e) => *error = Some(e),
                                }
                            }
                        });
                    });
                });
            }

            Dialog::Command { conn, db, text, result, error, busy } => {
                win(&format!("Executar comando · {db}")).default_width(620.0).default_height(520.0).open(open).show(ctx, |ui| {
                    ui.label(RichText::new("db.runCommand(…)").weak().monospace());
                    let out = CodeEditor::new(Id::unique(("cmd", uid)), text, EditorKind::Command, &[]).rows(4).show(ui);
                    ui.horizontal(|ui| {
                        let go = ui.add_enabled(!*busy, egui::Button::new(RichText::new("▶ Executar").strong())).on_hover_text("Ctrl+Enter").clicked();
                        if *busy {
                            ui.spinner();
                        }
                        if (go || out.submit) && !*busy {
                            match (shell::parse_document(text), self.client(conn)) {
                                (Ok(cmd), Some(client)) if !cmd.is_empty() => {
                                    *busy = true;
                                    *error = None;
                                    self.backend.spawn(mongo::run_command(client, db.clone(), cmd), move |app, res| {
                                        if let Some(Dialog::Command { result, error, busy, .. }) = app.dialog_mut(uid) {
                                            *busy = false;
                                            match res {
                                                Ok(d) => *result = Some(shell::doc_to_shell_pretty(&d)),
                                                Err(e) => *error = Some(e),
                                            }
                                        }
                                    });
                                }
                                (Err(e), _) => *error = Some(e.to_string()),
                                (_, None) => *error = Some("Conexão não está ativa.".into()),
                                _ => {}
                            }
                        }
                        if let Some(r) = result
                            && ui.button("Copiar resultado").clicked() {
                                ui.ctx().copy_text(r.clone());
                            }
                    });
                    error_label(ui, error);
                    if let Some(r) = result {
                        ui.separator();
                        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                            code_view(ui, r);
                        });
                    }
                });
            }

            Dialog::Explain { title, summary, warning, raw } => {
                win(title).default_width(640.0).default_height(560.0).open(open).show(ctx, |ui| {
                    if let Some(w) = warning {
                        ui.add(egui::Label::new(RichText::new(format!("⚠ {w}")).color(ui.visuals().warn_fg_color)).wrap());
                        ui.add_space(4.0);
                    }
                    egui::Grid::new(("explain", uid)).num_columns(2).striped(true).spacing([16.0, 6.0]).show(ui, |ui| {
                        for (k, v) in summary.iter() {
                            ui.label(RichText::new(k.as_str()).strong());
                            ui.label(RichText::new(v.as_str()).monospace());
                            ui.end_row();
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Resultado completo").strong());
                        if ui.small_button("Copiar").clicked() {
                            ui.ctx().copy_text(raw.clone());
                        }
                    });
                    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                        code_view(ui, raw);
                    });
                });
            }

            Dialog::Help => {
                win("Atalhos de teclado").default_width(520.0).open(open).show(ctx, |ui| {
                    egui::Grid::new("help").num_columns(2).striped(true).spacing([16.0, 6.0]).show(ui, |ui| {
                        for (k, v) in [
                            ("Enter (no filtro)", "Executar consulta"),
                            ("Shift+Enter (no filtro)", "Quebrar linha"),
                            ("Ctrl+Enter", "Executar pipeline / salvar no editor"),
                            ("Ctrl+Espaço", "Mostrar sugestões de autocomplete"),
                            ("⬆ ⬇  Enter/Tab  Esc", "Navegar/aceitar/fechar sugestões"),
                            ("Ctrl+F", "Focar o filtro"),
                            ("⬆ ⬇ PgUp PgDn", "Mover seleção na tabela (atravessa páginas)"),
                            ("Home / End", "Primeira / última linha da página"),
                            ("Ctrl+Home / Ctrl+End", "Primeiro / último documento da coleção"),
                            ("Alt+⬅ / Alt+➡", "Página anterior / próxima"),
                            ("Enter (na tabela)", "Abrir documento selecionado"),
                            ("Delete (na tabela)", "Excluir documento selecionado"),
                            ("Ctrl+C (na tabela)", "Copiar documento selecionado"),
                            ("Duplo clique na célula", "Editar o campo"),
                            ("Clique no cabeçalho", "Ordenar (Shift: ordenação múltipla)"),
                            ("F5", "Recarregar página"),
                            ("Ctrl+W / Ctrl+Tab", "Fechar aba / próxima aba"),
                            ("Ctrl + / Ctrl − / Ctrl 0", "Zoom"),
                            ("Esc", "Fechar diálogo"),
                        ] {
                            ui.label(RichText::new(k).monospace().strong());
                            ui.label(v);
                            ui.end_row();
                        }
                    });
                });
            }
        }
        outcome
    }

    fn export_request(&self, tab_id: u64, source: ExportSource, all: bool) -> Result<(mongodb::Client, Ns, mongo::ExportSource, String), String> {
        let tab = self.tab(tab_id).ok_or("aba fechada")?;
        let name = format!("{}.{}", tab.ns.db, tab.ns.coll);
        let src = match source {
            ExportSource::Query => {
                let mut spec = tab.docs.applied.clone().ok_or("Execute a consulta primeiro.")?;
                if all {
                    spec.skip = 0;
                    spec.limit = 0;
                } else {
                    spec.skip = tab.docs.paged.page * tab.docs.paged.page_size;
                    spec.limit = tab.docs.paged.page_size;
                }
                mongo::ExportSource::Find(Box::new(spec))
            }
            ExportSource::Aggregation => {
                let mut p = shell::parse_pipeline(&tab.agg.pipeline).map_err(|e| format!("Pipeline: {e}"))?;
                if mongo::pipeline_writes(&p) {
                    return Err("Não é possível exportar um pipeline com $out/$merge.".into());
                }
                if !all {
                    let pg = &tab.agg.paged;
                    p.push(doc! { "$skip": (pg.page * pg.page_size) as i64 });
                    p.push(doc! { "$limit": pg.page_size as i64 });
                }
                mongo::ExportSource::Aggregate(p)
            }
        };
        Ok((tab.client.clone(), tab.ns.clone(), src, name))
    }

    fn save_document(&mut self, uid: u64, tab_id: u64, mode: DocMode, text: &str, error: &mut Option<String>, busy: &mut bool) {
        let Some(tab) = self.tab(tab_id) else {
            *error = Some("A aba foi fechada.".into());
            return;
        };
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        let on_err = move |app: &mut App, e: String| {
            if let Some(Dialog::Document { error, busy, .. }) = app.dialog_mut(uid) {
                *error = Some(e);
                *busy = false;
            }
        };
        match mode {
            DocMode::Edit { id } => {
                let mut d = match shell::parse_document(text) {
                    Ok(d) => d,
                    Err(e) => {
                        *error = Some(e.to_string());
                        return;
                    }
                };
                match d.get("_id") {
                    Some(new_id) if *new_id != id => {
                        *error = Some("O _id não pode ser alterado. Para outro _id, use Clonar.".into());
                        return;
                    }
                    Some(_) => {}
                    None => {
                        let mut with_id = doc! { "_id": id.clone() };
                        with_id.extend(d);
                        d = with_id;
                    }
                }
                *busy = true;
                self.backend.spawn(mongo::replace_doc(client, ns, id, d), move |app, res| match res {
                    Ok(_) => {
                        app.close_dialog(uid);
                        app.toast_ok("Documento salvo.");
                        app.reload_docs(tab_id, false);
                    }
                    Err(e) => on_err(app, e),
                });
            }
            DocMode::Insert => {
                let docs = match shell::parse_documents(text) {
                    Ok(d) if d.is_empty() => {
                        *error = Some("Nada para inserir.".into());
                        return;
                    }
                    Ok(d) => d,
                    Err(e) => {
                        *error = Some(e.to_string());
                        return;
                    }
                };
                *busy = true;
                self.backend.spawn(mongo::insert_docs(client, ns, docs), move |app, res| match res {
                    Ok(n) => {
                        app.close_dialog(uid);
                        app.toast_ok(format!("{n} documento(s) inserido(s)."));
                        app.reload_docs(tab_id, true);
                        app.load_stats(tab_id);
                    }
                    Err(e) => on_err(app, e),
                });
            }
            DocMode::View => {}
        }
    }

    /// Executa a ação confirmada. Retorna `true` se o diálogo pode fechar imediatamente.
    fn run_confirm(&mut self, uid: u64, action: ConfirmAction) -> bool {
        let fail = move |app: &mut App, e: String| {
            if let Some(Dialog::Confirm { error, busy, .. }) = app.dialog_mut(uid) {
                *error = Some(e);
                *busy = false;
            }
        };
        match action {
            ConfirmAction::RemoveConnection(id) => {
                self.disconnect(&id);
                self.conns.remove(&id);
                self.config.connections.retain(|c| c.id != id);
                self.save_config();
                true
            }
            ConfirmAction::DropDatabase { conn, db } => {
                let Some(client) = self.client(&conn) else { return true };
                self.backend.spawn(mongo::drop_database(client, db.clone()), move |app, res| match res {
                    Ok(()) => {
                        app.close_dialog(uid);
                        app.tabs.retain(|t| !(t.conn_id == conn && t.ns.db == db));
                        app.active = app.active.min(app.tabs.len().saturating_sub(1));
                        app.toast_ok(format!("Banco {db} excluído."));
                        app.refresh_conn(&conn);
                    }
                    Err(e) => fail(app, e),
                });
                false
            }
            ConfirmAction::DropCollection { conn, db, coll } => {
                let Some(client) = self.client(&conn) else { return true };
                self.backend.spawn(mongo::drop_collection(client, Ns::new(db.clone(), coll.clone())), move |app, res| match res {
                    Ok(()) => {
                        app.close_dialog(uid);
                        app.tabs.retain(|t| !(t.conn_id == conn && t.ns.db == db && t.ns.coll == coll));
                        app.active = app.active.min(app.tabs.len().saturating_sub(1));
                        app.toast_ok(format!("Coleção {db}.{coll} excluída."));
                        app.load_collections(&conn, &db);
                    }
                    Err(e) => fail(app, e),
                });
                false
            }
            ConfirmAction::DropIndex { tab, name } => {
                let Some(t) = self.tab(tab) else { return true };
                let (client, ns) = (t.client.clone(), t.ns.clone());
                self.backend.spawn(mongo::drop_index(client, ns, name.clone()), move |app, res| match res {
                    Ok(()) => {
                        app.close_dialog(uid);
                        app.toast_ok(format!("Índice {name} excluído."));
                        app.load_indexes(tab);
                        app.load_stats(tab);
                    }
                    Err(e) => fail(app, e),
                });
                false
            }
            ConfirmAction::DeleteDocument { tab, id } => {
                let Some(t) = self.tab(tab) else { return true };
                let (client, ns) = (t.client.clone(), t.ns.clone());
                self.backend.spawn(mongo::delete_by_id(client, ns, id), move |app, res| match res {
                    Ok(n) => {
                        app.close_dialog(uid);
                        app.toast_ok(if n > 0 { "Documento excluído." } else { "O documento já não existia." });
                        app.reload_docs(tab, true);
                        app.load_stats(tab);
                    }
                    Err(e) => fail(app, e),
                });
                false
            }
        }
    }
}
