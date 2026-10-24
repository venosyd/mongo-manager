//! Estado da aplicação, barra lateral de conexões, abas e infraestrutura assíncrona.

use crate::config::{Config, SavedConnection};
use crate::dialogs::Dialog;
use crate::grid::{GridData, GridState};
use crate::mongo::{self, CollInfo, CollKind, DbInfo, FieldInfo, FindSpec, IndexList, Ns};
use bson::Document;
use egui::{Color32, RichText, Ui};
use mongodb::Client;
use std::collections::HashMap;
use std::future::Future;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub type Reply = Box<dyn FnOnce(&mut App) + Send>;

pub struct Backend {
    rt: tokio::runtime::Runtime,
    tx: mpsc::Sender<Reply>,
    ctx: egui::Context,
}

impl Backend {
    /// Executa `fut` no runtime e entrega o resultado a `done` na thread da interface.
    pub fn spawn<T, F, H>(&self, fut: F, done: H)
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
        H: FnOnce(&mut App, T) + Send + 'static,
    {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        self.rt.spawn(async move {
            let v = fut.await;
            let _ = tx.send(Box::new(move |app: &mut App| done(app, v)));
            ctx.request_repaint();
        });
    }

    pub fn spawn_blocking<T, F, H>(&self, f: F, done: H)
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
        H: FnOnce(&mut App, T) + Send + 'static,
    {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        self.rt.spawn_blocking(move || {
            let v = f();
            let _ = tx.send(Box::new(move |app: &mut App| done(app, v)));
            ctx.request_repaint();
        });
    }
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub until: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConnStatus {
    Idle,
    Connecting,
    Connected,
    Failed(String),
}

pub struct DbNode {
    pub info: DbInfo,
    pub colls: Option<Vec<CollInfo>>,
    pub loading: bool,
    pub open: bool,
    pub error: Option<String>,
}

pub struct ConnRuntime {
    pub status: ConnStatus,
    pub client: Option<Client>,
    pub dbs: Vec<DbNode>,
    pub default_db: Option<String>,
    pub version: String,
    pub open: bool,
}

impl Default for ConnRuntime {
    fn default() -> Self {
        Self {
            status: ConnStatus::Idle,
            client: None,
            dbs: Vec::new(),
            default_db: None,
            version: String::new(),
            open: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Documents,
    Aggregation,
    Schema,
    Indexes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Table,
    Json,
}

/// Resultado exibido (página atual).
pub struct ResultView {
    pub grid: GridData,
    pub elapsed_ms: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingSelect {
    First,
    Last,
}

/// Paginação e resultados compartilhados entre a aba Documentos e a aba Agregação.
pub struct Paged {
    pub page: u64,
    pub page_size: u64,
    pub total: Option<u64>,
    pub counting: bool,
    pub count_error: Option<String>,
    pub loading: bool,
    pub page_seq: u64,
    pub count_seq: u64,
    pub result: Option<ResultView>,
    pub error: Option<String>,
    pub mode: ViewMode,
    pub grid: GridState,
    pub page_input: String,
    pub pending_select: Option<PendingSelect>,
    /// "Última página" pedida antes de a contagem terminar.
    pub want_last: bool,
}

impl Paged {
    pub fn new(page_size: u64) -> Self {
        Self {
            page: 0,
            page_size,
            total: None,
            counting: false,
            count_error: None,
            loading: false,
            page_seq: 0,
            count_seq: 0,
            result: None,
            error: None,
            mode: ViewMode::Table,
            grid: GridState::default(),
            page_input: "1".into(),
            pending_select: None,
            want_last: false,
        }
    }

    pub fn last_page(&self) -> Option<u64> {
        self.total.map(|t| t.saturating_sub(1) / self.page_size.max(1))
    }

    pub fn has_next(&self) -> bool {
        match self.last_page() {
            Some(last) => self.page < last,
            None => self
                .result
                .as_ref()
                .is_some_and(|r| r.grid.docs.len() as u64 >= self.page_size),
        }
    }
}

pub struct DocsState {
    pub filter: String,
    pub project: String,
    pub sort: String,
    pub hint: String,
    pub show_options: bool,
    pub applied: Option<FindSpec>,
    /// Ordenação como o usuário escreveu (sem o desempate por _id), para as setas do cabeçalho.
    pub applied_sort: Document,
    pub paged: Paged,
    pub show_detail: bool,
}

pub struct AggState {
    pub pipeline: String,
    pub applied: Option<Vec<Document>>,
    pub paged: Paged,
    pub save_name: String,
    pub writes: bool,
}

#[derive(Default)]
pub struct IndexState {
    pub loading: bool,
    pub list: Option<IndexList>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct SchemaState {
    pub loading: bool,
    pub sampled: usize,
    pub error: Option<String>,
}

pub struct Tab {
    pub id: u64,
    pub conn_id: String,
    pub conn_name: String,
    pub client: Client,
    pub ns: Ns,
    pub kind: CollKind,
    pub view: View,
    pub docs: DocsState,
    pub agg: AggState,
    pub idx: IndexState,
    pub schema: SchemaState,
    pub fields: Vec<String>,
    pub field_info: Vec<FieldInfo>,
    pub stats: Option<Document>,
}

impl Tab {
    pub fn history_key(&self) -> String {
        format!("{}/{}", self.conn_id, self.ns)
    }

    pub fn editable(&self) -> bool {
        self.kind == CollKind::Collection
    }
}

enum SideAction {
    ToggleConn(String),
    Connect(String),
    Disconnect(String),
    EditConn(String),
    DuplicateConn(String),
    RemoveConn(String),
    RefreshConn(String),
    ToggleDb(String, String),
    RefreshDb(String, String),
    OpenColl { conn: String, db: String, coll: String, kind: CollKind, new_tab: bool },
    CreateColl(String, Option<String>),
    DropDb(String, String),
    DropColl(String, String, String),
    RenameColl(String, String, String),
    RunCommand(String, String),
}

pub struct App {
    pub backend: Backend,
    rx: mpsc::Receiver<Reply>,
    pub config: Config,
    pub conns: HashMap<String, ConnRuntime>,
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_id: u64,
    pub dialogs: Vec<(u64, Dialog)>,
    /// Diálogos que já receberam o foco inicial.
    pub autofocused: HashMap<u64, u8>,
    pub toasts: Vec<Toast>,
    tree_filter: String,
    applied_zoom: f32,
    screenshot: Option<(String, Instant)>,
    shot_counter: u32,
    script: Option<crate::script::Script>,
}

pub fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

pub fn fmt_bytes(n: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{v:.0} {}", units[u]) } else { format!("{v:.1} {}", units[u]) }
}

pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0xED, 0x64);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(0x00, 0x68, 0x4A);

fn apply_style(ctx: &egui::Context, dark: bool) {
    use egui::{FontFamily, FontId, TextStyle};
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.interact_size.y = 24.0;
    });
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.visuals.selection.bg_fill = ACCENT_DARK;
        s.visuals.selection.stroke.color = Color32::WHITE;
        s.visuals.hyperlink_color = ACCENT;
    });
    ctx.style_mut_of(egui::Theme::Light, |s| {
        s.visuals.selection.bg_fill = Color32::from_rgb(0xB8, 0xEC, 0xCF);
        s.visuals.selection.stroke.color = Color32::from_rgb(0x00, 0x1E, 0x2B);
        s.visuals.hyperlink_color = ACCENT_DARK;
    });
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("runtime tokio");
        let first_run = !Config::path().exists();
        let mut config = Config::load();
        if first_run && config.connections.is_empty() {
            config.connections.push(SavedConnection {
                id: uuid::Uuid::new_v4().to_string(),
                name: "localhost".into(),
                uri: "mongodb://localhost:27017".into(),
            });
        }
        apply_style(&cc.egui_ctx, config.settings.dark_mode);
        let screenshot = std::env::var("MM_SCREENSHOT").ok().map(|p| {
            let ms = std::env::var("MM_SCREENSHOT_DELAY").ok().and_then(|f| f.parse().ok()).unwrap_or(3000);
            (p, Instant::now() + Duration::from_millis(ms))
        });
        let mut app = Self {
            backend: Backend {
                rt,
                tx,
                ctx: cc.egui_ctx.clone(),
            },
            rx,
            config,
            conns: HashMap::new(),
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            dialogs: Vec::new(),
            autofocused: Default::default(),
            toasts: Vec::new(),
            tree_filter: String::new(),
            applied_zoom: 0.0,
            screenshot,
            shot_counter: 0,
            script: std::env::var("MM_SCRIPT").ok().and_then(|p| crate::script::Script::load(&p)),
        };
        // Atalho para testes/automação: conecta e abre uma coleção ao iniciar.
        if let Ok(spec) = std::env::var("MM_OPEN") {
            app.auto_open(spec);
        }
        app
    }

    fn auto_open(&mut self, spec: String) {
        let wanted = std::env::var("MM_CONN").ok();
        let Some(conn) = self
            .config
            .connections
            .iter()
            .find(|c| wanted.as_ref().is_none_or(|w| *w == c.id))
            .cloned()
        else {
            return;
        };
        self.connect(&conn.id);
        let (db, coll) = spec.split_once('.').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((spec, String::new()));
        let id = conn.id.clone();
        self.backend.spawn(async { tokio::time::sleep(Duration::from_millis(1500)).await }, move |app, _| {
            if coll.is_empty() {
                return;
            }
            app.open_collection(&id, &db, &coll, CollKind::Collection, false);
            if let Ok(view) = std::env::var("MM_VIEW")
                && let Some(t) = app.tabs.last_mut() {
                    t.view = match view.as_str() {
                        "agg" => View::Aggregation,
                        "schema" => View::Schema,
                        "indexes" => View::Indexes,
                        _ => View::Documents,
                    };
                }
        });
    }

    pub fn next_uid(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn save_config(&mut self) {
        if let Err(e) = self.config.save() {
            self.toast_err(format!("Não foi possível salvar a configuração: {e}"));
        }
    }

    pub fn toast_ok(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            error: false,
            until: Instant::now() + Duration::from_secs(4),
        });
    }

    pub fn toast_err(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            error: true,
            until: Instant::now() + Duration::from_secs(10),
        });
    }

    pub fn open_dialog(&mut self, d: Dialog) {
        let uid = self.next_uid();
        self.dialogs.push((uid, d));
    }

    pub fn dialog_mut(&mut self, uid: u64) -> Option<&mut Dialog> {
        self.dialogs.iter_mut().find(|(u, _)| *u == uid).map(|(_, d)| d)
    }

    pub fn close_dialog(&mut self, uid: u64) {
        self.dialogs.retain(|(u, _)| *u != uid);
    }

    pub fn tab_mut(&mut self, id: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn tab(&self, id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn conn_name(&self, id: &str) -> String {
        self.config
            .connections
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }

    pub fn client(&self, conn: &str) -> Option<Client> {
        self.conns.get(conn).and_then(|c| c.client.clone())
    }

    // -----------------------------------------------------------------------
    // Conexões e árvore

    pub fn connect(&mut self, id: &str) {
        let Some(saved) = self.config.connections.iter().find(|c| c.id == id).cloned() else { return };
        let rt = self.conns.entry(id.to_string()).or_default();
        if rt.status == ConnStatus::Connecting {
            return;
        }
        rt.status = ConnStatus::Connecting;
        rt.open = true;
        let id = id.to_string();
        self.backend.spawn(mongo::connect(saved.uri.clone()), move |app, res| {
            let rt = app.conns.entry(id.clone()).or_default();
            match res {
                Ok(c) => {
                    rt.status = ConnStatus::Connected;
                    rt.client = Some(c.client);
                    rt.default_db = c.default_db;
                    rt.version = c.version;
                    rt.dbs = c
                        .dbs
                        .into_iter()
                        .map(|info| DbNode {
                            info,
                            colls: None,
                            loading: false,
                            open: false,
                            error: None,
                        })
                        .collect();
                    // Com um único banco (ex.: URI com banco padrão), já expande.
                    if rt.dbs.len() == 1 {
                        let db = rt.dbs[0].info.name.clone();
                        app.toggle_db(&id, &db);
                    }
                }
                Err(e) => {
                    rt.status = ConnStatus::Failed(e.clone());
                    rt.client = None;
                    let name = app.conn_name(&id);
                    app.toast_err(format!("{name}: {e}"));
                }
            }
        });
    }

    pub fn disconnect(&mut self, id: &str) {
        if let Some(rt) = self.conns.get_mut(id) {
            if let Some(client) = rt.client.take() {
                self.backend.rt.spawn(async move { client.shutdown().await });
            }
            *rt = ConnRuntime::default();
        }
        let before = self.tabs.len();
        self.tabs.retain(|t| t.conn_id != id);
        if self.tabs.len() != before {
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
    }

    pub fn refresh_conn(&mut self, id: &str) {
        let Some(rt) = self.conns.get(id) else { return };
        let Some(client) = rt.client.clone() else {
            self.connect(id);
            return;
        };
        let default_db = rt.default_db.clone();
        let id = id.to_string();
        self.backend.spawn(mongo::list_databases(client, default_db), move |app, res| match res {
            Ok(dbs) => {
                let mut reload = Vec::new();
                if let Some(rt) = app.conns.get_mut(&id) {
                    let old: HashMap<String, DbNode> = rt.dbs.drain(..).map(|d| (d.info.name.clone(), d)).collect();
                    let mut old = old;
                    rt.dbs = dbs
                        .into_iter()
                        .map(|info| match old.remove(&info.name) {
                            Some(mut n) => {
                                if n.open {
                                    reload.push(info.name.clone());
                                }
                                n.info = info;
                                n
                            }
                            None => DbNode {
                                info,
                                colls: None,
                                loading: false,
                                open: false,
                                error: None,
                            },
                        })
                        .collect();
                }
                for db in reload {
                    app.load_collections(&id, &db);
                }
            }
            Err(e) => app.toast_err(e),
        });
    }

    pub fn load_collections(&mut self, conn: &str, db: &str) {
        let Some(client) = self.client(conn) else { return };
        if let Some(node) = self
            .conns
            .get_mut(conn)
            .and_then(|r| r.dbs.iter_mut().find(|d| d.info.name == db))
        {
            node.loading = true;
        }
        let (conn, db) = (conn.to_string(), db.to_string());
        self.backend.spawn(mongo::list_collections(client, db.clone()), move |app, res| {
            if let Some(node) = app
                .conns
                .get_mut(&conn)
                .and_then(|r| r.dbs.iter_mut().find(|d| d.info.name == db))
            {
                node.loading = false;
                match res {
                    Ok(c) => {
                        node.colls = Some(c);
                        node.error = None;
                    }
                    Err(e) => node.error = Some(e),
                }
            }
        });
    }

    pub fn toggle_db(&mut self, conn: &str, db: &str) {
        let mut load = false;
        if let Some(node) = self
            .conns
            .get_mut(conn)
            .and_then(|r| r.dbs.iter_mut().find(|d| d.info.name == db))
        {
            node.open = !node.open;
            load = node.open && node.colls.is_none() && !node.loading;
        }
        if load {
            self.load_collections(conn, db);
        }
    }

    pub fn open_collection(&mut self, conn: &str, db: &str, coll: &str, kind: CollKind, new_tab: bool) {
        let ns = Ns::new(db, coll);
        if !new_tab
            && let Some(i) = self.tabs.iter().position(|t| t.conn_id == conn && t.ns == ns) {
                self.active = i;
                return;
            }
        let Some(client) = self.client(conn) else { return };
        let id = self.next_uid();
        let page_size = self.config.settings.page_size;
        let tab = Tab {
            id,
            conn_id: conn.to_string(),
            conn_name: self.conn_name(conn),
            client,
            ns,
            kind,
            view: View::Documents,
            docs: DocsState {
                filter: String::new(),
                project: String::new(),
                sort: String::new(),
                hint: String::new(),
                show_options: false,
                applied: None,
                applied_sort: Document::new(),
                paged: Paged::new(page_size),
                show_detail: false,
            },
            agg: AggState {
                pipeline: "[\n  { $match: {} }\n]".into(),
                applied: None,
                paged: Paged::new(page_size.min(100)),
                save_name: String::new(),
                writes: false,
            },
            idx: IndexState::default(),
            schema: SchemaState::default(),
            fields: Vec::new(),
            field_info: Vec::new(),
            stats: None,
        };
        let mut tab = tab;
        if let Some(prefs) = self.config.columns.get(&tab.history_key()) {
            tab.docs.paged.grid.widths = prefs.widths.clone();
            tab.docs.paged.grid.hidden = prefs.hidden.iter().cloned().collect();
        }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.run_find(id);
        self.sample_fields(id);
        self.load_stats(id);
    }

    /// Guarda larguras e colunas ocultas das tabelas abertas na configuração.
    pub fn sync_column_prefs(&mut self) {
        for t in &self.tabs {
            let g = &t.docs.paged.grid;
            if g.widths.is_empty() && g.hidden.is_empty() {
                continue;
            }
            let mut hidden: Vec<String> = g.hidden.iter().cloned().collect();
            hidden.sort();
            self.config.columns.insert(
                t.history_key(),
                crate::config::ColumnPrefs {
                    widths: g.widths.iter().map(|(k, v)| (k.clone(), v.round())).collect(),
                    hidden,
                },
            );
        }
    }

    pub fn close_tab(&mut self, idx: usize) {
        self.sync_column_prefs();
        self.save_config();
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len().saturating_sub(1);
            } else if self.active > idx {
                self.active -= 1;
            }
        }
    }

    pub fn sample_fields(&mut self, tab_id: u64) {
        let size = self.config.settings.sample_size;
        let Some(tab) = self.tab_mut(tab_id) else { return };
        tab.schema.loading = true;
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        self.backend.spawn(mongo::sample_fields(client, ns, size), move |app, res| {
            let Some(tab) = app.tab_mut(tab_id) else { return };
            tab.schema.loading = false;
            match res {
                Ok((info, n)) => {
                    tab.fields = info.iter().map(|f| f.path.clone()).collect();
                    tab.field_info = info;
                    tab.schema.sampled = n;
                    tab.schema.error = None;
                }
                Err(e) => tab.schema.error = Some(e),
            }
        });
    }

    pub fn load_stats(&mut self, tab_id: u64) {
        let Some(tab) = self.tab(tab_id) else { return };
        let (client, ns) = (tab.client.clone(), tab.ns.clone());
        self.backend.spawn(mongo::coll_stats(client, ns), move |app, res| {
            if let (Some(tab), Ok(stats)) = (app.tab_mut(tab_id), res) {
                tab.stats = Some(stats);
            }
        });
    }

    // -----------------------------------------------------------------------
    // Interface principal

    fn process_replies(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            r(self);
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("🍃 Mongo Manager").strong().size(16.0).color(if ui.visuals().dark_mode { ACCENT } else { ACCENT_DARK }));
            ui.separator();
            if ui.button("➕ Nova conexão").clicked() {
                self.open_dialog(Dialog::new_connection());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("❓ Atalhos").clicked() {
                    self.open_dialog(Dialog::Help);
                }
                let dark = self.config.settings.dark_mode;
                if ui
                    .button(if dark { "☀ Tema claro" } else { "🌙 Tema escuro" })
                    .clicked()
                {
                    self.config.settings.dark_mode = !dark;
                    apply_style(ui.ctx(), !dark);
                    self.save_config();
                }
                ui.separator();
                if ui.small_button("A+").on_hover_text("Aumentar zoom (Ctrl +)").clicked() {
                    self.config.settings.zoom = (self.config.settings.zoom + 0.1).min(2.5);
                    self.save_config();
                }
                ui.label(format!("{:.0}%", self.config.settings.zoom * 100.0));
                if ui.small_button("A−").on_hover_text("Diminuir zoom (Ctrl −)").clicked() {
                    self.config.settings.zoom = (self.config.settings.zoom - 0.1).max(0.6);
                    self.save_config();
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if let Some(tab) = self.tabs.get(self.active) {
                let version = self.conns.get(&tab.conn_id).map(|c| c.version.clone()).unwrap_or_default();
                ui.label(RichText::new(format!("{} · MongoDB {} · {}", tab.conn_name, version, tab.ns)).small().weak());
            } else {
                let connected = self.conns.values().filter(|c| c.status == ConnStatus::Connected).count();
                ui.label(RichText::new(format!("{connected} conexão(ões) ativa(s)")).small().weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(Config::path().display().to_string()).small().weak())
                    .on_hover_text("Arquivo de configuração (conexões, histórico, pipelines salvos)");
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let mut actions = Vec::new();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("CONEXÕES").small().strong().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("➕").on_hover_text("Nova conexão").clicked() {
                    self.open_dialog(Dialog::new_connection());
                }
            });
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.tree_filter)
                .hint_text("🔍 Filtrar bancos e coleções")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        let filter = self.tree_filter.trim().to_lowercase();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.config.connections.is_empty() {
                ui.label(RichText::new("Nenhuma conexão. Clique em ➕ para adicionar.").weak());
            }
            for saved in &self.config.connections {
                let rt = self.conns.get(&saved.id);
                let status = rt.map(|r| r.status.clone()).unwrap_or(ConnStatus::Idle);
                let open = rt.is_some_and(|r| r.open);
                let (filled, color) = match &status {
                    ConnStatus::Connected => (true, ACCENT),
                    ConnStatus::Connecting => (false, Color32::from_rgb(0xF2, 0xC0, 0x37)),
                    ConnStatus::Failed(_) => (true, Color32::from_rgb(0xE5, 0x48, 0x4D)),
                    ConnStatus::Idle => (false, ui.visuals().weak_text_color()),
                };
                let resp = tree_row(ui, 0, Some(open && status == ConnStatus::Connected), |ui| {
                    status_dot(ui, filled, color);
                    ui.add(egui::Label::new(RichText::new(&saved.name).strong()).truncate().selectable(false));
                    if status == ConnStatus::Connecting {
                        ui.add(egui::Spinner::new().size(12.0));
                    }
                });
                let resp = match &status {
                    ConnStatus::Failed(e) => resp.on_hover_text(format!("Falha: {e}")),
                    _ => resp.on_hover_text(redact_uri(&saved.uri)),
                };
                if resp.clicked() {
                    actions.push(SideAction::ToggleConn(saved.id.clone()));
                }
                resp.context_menu(|ui| {
                    if status == ConnStatus::Connected {
                        if ui.button("⟳ Atualizar").clicked() {
                            actions.push(SideAction::RefreshConn(saved.id.clone()));
                            ui.close();
                        }
                        if ui.button("➕ Criar banco de dados…").clicked() {
                            actions.push(SideAction::CreateColl(saved.id.clone(), None));
                            ui.close();
                        }
                        if ui.button("⌨ Executar comando (admin)…").clicked() {
                            actions.push(SideAction::RunCommand(saved.id.clone(), "admin".into()));
                            ui.close();
                        }
                        if ui.button("Desconectar").clicked() {
                            actions.push(SideAction::Disconnect(saved.id.clone()));
                            ui.close();
                        }
                    } else if ui.button("Conectar").clicked() {
                        actions.push(SideAction::Connect(saved.id.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("✏ Editar conexão…").clicked() {
                        actions.push(SideAction::EditConn(saved.id.clone()));
                        ui.close();
                    }
                    if ui.button("🗐 Duplicar").clicked() {
                        actions.push(SideAction::DuplicateConn(saved.id.clone()));
                        ui.close();
                    }
                    if ui.button("Copiar URI").clicked() {
                        ui.ctx().copy_text(saved.uri.clone());
                        ui.close();
                    }
                    if ui.button(RichText::new("🗑 Remover").color(ui.visuals().error_fg_color)).clicked() {
                        actions.push(SideAction::RemoveConn(saved.id.clone()));
                        ui.close();
                    }
                });

                let Some(rt) = rt else { continue };
                if !(open && status == ConnStatus::Connected) {
                    continue;
                }
                for db in &rt.dbs {
                    let db_name = &db.info.name;
                    let db_match = filter.is_empty() || db_name.to_lowercase().contains(&filter);
                    let coll_matches: Vec<&CollInfo> = db
                        .colls
                        .iter()
                        .flatten()
                        .filter(|c| filter.is_empty() || db_match || c.name.to_lowercase().contains(&filter))
                        .collect();
                    if !db_match && coll_matches.is_empty() {
                        continue;
                    }
                    let expanded = db.open || (!filter.is_empty() && !coll_matches.is_empty() && !db_match);
                    let resp = tree_row(ui, 1, Some(expanded), |ui| {
                        ui.label("🗄");
                        ui.add(egui::Label::new(db_name.as_str()).truncate().selectable(false));
                        if db.loading {
                            ui.add(egui::Spinner::new().size(12.0));
                        }
                        if db.info.size_on_disk > 0 {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(fmt_bytes(db.info.size_on_disk as f64)).small().weak());
                            });
                        }
                    });
                    if resp.clicked() {
                        actions.push(SideAction::ToggleDb(saved.id.clone(), db_name.clone()));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("➕ Criar coleção…").clicked() {
                            actions.push(SideAction::CreateColl(saved.id.clone(), Some(db_name.clone())));
                            ui.close();
                        }
                        if ui.button("⟳ Atualizar").clicked() {
                            actions.push(SideAction::RefreshDb(saved.id.clone(), db_name.clone()));
                            ui.close();
                        }
                        if ui.button("⌨ Executar comando…").clicked() {
                            actions.push(SideAction::RunCommand(saved.id.clone(), db_name.clone()));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(RichText::new("🗑 Excluir banco…").color(ui.visuals().error_fg_color)).clicked() {
                            actions.push(SideAction::DropDb(saved.id.clone(), db_name.clone()));
                            ui.close();
                        }
                    });
                    if !expanded {
                        continue;
                    }
                    if let Some(e) = &db.error {
                        ui.horizontal(|ui| {
                            ui.add_space(40.0);
                            ui.label(RichText::new(e).small().color(ui.visuals().error_fg_color));
                        });
                    }
                    if db.colls.as_ref().is_some_and(|c| c.is_empty()) {
                        ui.horizontal(|ui| {
                            ui.add_space(40.0);
                            ui.label(RichText::new("(sem coleções)").small().weak());
                        });
                    }
                    for c in coll_matches {
                        let active = self
                            .tabs
                            .get(self.active)
                            .is_some_and(|t| t.conn_id == saved.id && t.ns.db == *db_name && t.ns.coll == c.name);
                        let icon = match c.kind {
                            CollKind::Collection => "📄",
                            CollKind::View => "👁",
                            CollKind::Timeseries => "⏱",
                        };
                        let resp = tree_row_selected(ui, 2, active, |ui| {
                            ui.label(RichText::new(icon).small());
                            ui.add(egui::Label::new(c.name.as_str()).truncate().selectable(false));
                        });
                        if resp.clicked() {
                            actions.push(SideAction::OpenColl {
                                conn: saved.id.clone(),
                                db: db_name.clone(),
                                coll: c.name.clone(),
                                kind: c.kind,
                                new_tab: false,
                            });
                        }
                        if resp.middle_clicked() {
                            actions.push(SideAction::OpenColl {
                                conn: saved.id.clone(),
                                db: db_name.clone(),
                                coll: c.name.clone(),
                                kind: c.kind,
                                new_tab: true,
                            });
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Abrir").clicked() {
                                actions.push(SideAction::OpenColl {
                                    conn: saved.id.clone(),
                                    db: db_name.clone(),
                                    coll: c.name.clone(),
                                    kind: c.kind,
                                    new_tab: false,
                                });
                                ui.close();
                            }
                            if ui.button("Abrir em nova aba").clicked() {
                                actions.push(SideAction::OpenColl {
                                    conn: saved.id.clone(),
                                    db: db_name.clone(),
                                    coll: c.name.clone(),
                                    kind: c.kind,
                                    new_tab: true,
                                });
                                ui.close();
                            }
                            if ui.button("Copiar nome").clicked() {
                                ui.ctx().copy_text(c.name.clone());
                                ui.close();
                            }
                            ui.separator();
                            if c.kind == CollKind::Collection && ui.button("✏ Renomear…").clicked() {
                                actions.push(SideAction::RenameColl(saved.id.clone(), db_name.clone(), c.name.clone()));
                                ui.close();
                            }
                            if ui.button(RichText::new("🗑 Excluir coleção…").color(ui.visuals().error_fg_color)).clicked() {
                                actions.push(SideAction::DropColl(saved.id.clone(), db_name.clone(), c.name.clone()));
                                ui.close();
                            }
                        });
                    }
                }
            }
        });

        for a in actions {
            self.apply_side_action(a);
        }
    }

    fn apply_side_action(&mut self, a: SideAction) {
        match a {
            SideAction::ToggleConn(id) => {
                let status = self.conns.get(&id).map(|r| r.status.clone()).unwrap_or(ConnStatus::Idle);
                match status {
                    ConnStatus::Connected => {
                        if let Some(rt) = self.conns.get_mut(&id) {
                            rt.open = !rt.open;
                        }
                    }
                    ConnStatus::Connecting => {}
                    _ => self.connect(&id),
                }
            }
            SideAction::Connect(id) => self.connect(&id),
            SideAction::Disconnect(id) => self.disconnect(&id),
            SideAction::RefreshConn(id) => self.refresh_conn(&id),
            SideAction::EditConn(id) => {
                if let Some(c) = self.config.connections.iter().find(|c| c.id == id).cloned() {
                    self.open_dialog(Dialog::edit_connection(&c));
                }
            }
            SideAction::DuplicateConn(id) => {
                if let Some(mut c) = self.config.connections.iter().find(|c| c.id == id).cloned() {
                    c.id = uuid::Uuid::new_v4().to_string();
                    c.name = format!("{} (cópia)", c.name);
                    self.config.connections.push(c);
                    self.save_config();
                }
            }
            SideAction::RemoveConn(id) => {
                let name = self.conn_name(&id);
                self.open_dialog(Dialog::confirm(
                    "Remover conexão",
                    format!("Remover a conexão \"{name}\" da lista? (Os dados no servidor não são afetados.)"),
                    "Remover",
                    crate::dialogs::ConfirmAction::RemoveConnection(id),
                ));
            }
            SideAction::ToggleDb(conn, db) => self.toggle_db(&conn, &db),
            SideAction::RefreshDb(conn, db) => self.load_collections(&conn, &db),
            SideAction::OpenColl { conn, db, coll, kind, new_tab } => self.open_collection(&conn, &db, &coll, kind, new_tab),
            SideAction::CreateColl(conn, db) => self.open_dialog(Dialog::create_collection(conn, db)),
            SideAction::DropDb(conn, db) => self.open_dialog(Dialog::confirm(
                "Excluir banco de dados",
                format!("Excluir o banco \"{db}\" e TODAS as suas coleções? Esta ação não pode ser desfeita."),
                "Excluir banco",
                crate::dialogs::ConfirmAction::DropDatabase { conn, db },
            )),
            SideAction::DropColl(conn, db, coll) => self.open_dialog(Dialog::confirm(
                "Excluir coleção",
                format!("Excluir a coleção \"{db}.{coll}\" e todos os seus documentos e índices? Esta ação não pode ser desfeita."),
                "Excluir coleção",
                crate::dialogs::ConfirmAction::DropCollection { conn, db, coll },
            )),
            SideAction::RenameColl(conn, db, coll) => self.open_dialog(Dialog::rename(conn, db, coll)),
            SideAction::RunCommand(conn, db) => self.open_dialog(Dialog::command(conn, db)),
        }
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        let mut close: Option<usize> = None;
        let mut activate: Option<usize> = None;
        egui::ScrollArea::horizontal()
            .id_salt("tabbar")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let active = i == self.active;
                        let frame = egui::Frame::new()
                            .fill(if active { ui.visuals().extreme_bg_color } else { ui.visuals().faint_bg_color })
                            .stroke(if active {
                                egui::Stroke::new(1.0, if ui.visuals().dark_mode { ACCENT } else { ACCENT_DARK })
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke
                            })
                            .corner_radius(egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 })
                            .inner_margin(egui::Margin::symmetric(8, 4));
                        let inner = frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let icon = match tab.kind {
                                    CollKind::View => "👁 ",
                                    _ => "",
                                };
                                let text = RichText::new(format!("{icon}{}", tab.ns));
                                let r = ui.add(egui::Label::new(if active { text.strong() } else { text }).selectable(false).sense(egui::Sense::click()));
                                let x = ui.add(egui::Button::new(RichText::new("✖").small()).frame(false));
                                (r, x)
                            })
                            .inner
                        });
                        let (label, x) = inner.inner;
                        let whole = inner.response.interact(egui::Sense::click());
                        if label.clicked() || whole.clicked() {
                            activate = Some(i);
                        }
                        if label.middle_clicked() || whole.middle_clicked() || x.clicked() {
                            close = Some(i);
                        }
                        label.on_hover_text(format!("{} — {}", tab.conn_name, tab.ns));
                    }
                });
            });
        if let Some(i) = activate {
            self.active = i;
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.2);
            ui.label(RichText::new("🍃").size(48.0));
            ui.heading("Mongo Manager");
            ui.label(RichText::new("Selecione uma coleção na barra lateral para começar.").weak());
            ui.add_space(12.0);
            if ui.button("➕ Nova conexão").clicked() {
                self.open_dialog(Dialog::new_connection());
            }
            ui.add_space(20.0);
            ui.label(RichText::new("Dicas").strong());
            for tip in [
                "Clique no cabeçalho de uma coluna para ordenar; arraste a borda para ajustar a largura.",
                "Ctrl+End vai para o último documento da coleção; Ctrl+Home volta ao primeiro.",
                "Duplo clique numa célula edita o campo; Enter abre o documento inteiro.",
                "Nos filtros, Ctrl+Espaço mostra as sugestões de campos e operadores.",
            ] {
                ui.label(RichText::new(format!("• {tip}")).weak());
            }
        });
    }

    fn show_toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts.retain(|t| t.until > now);
        if self.toasts.is_empty() {
            return;
        }
        let mut dismiss = None;
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -32.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                for (i, t) in self.toasts.iter().enumerate().rev().take(5) {
                    let (bg, fg) = if t.error {
                        (Color32::from_rgb(0x5C, 0x1A, 0x1D), Color32::from_rgb(0xFF, 0xD7, 0xD9))
                    } else {
                        (Color32::from_rgb(0x0B, 0x3B, 0x2C), Color32::from_rgb(0xD6, 0xFF, 0xE9))
                    };
                    let r = egui::Frame::new()
                        .fill(bg)
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(&t.text).color(fg)).wrap());
                        })
                        .response
                        .interact(egui::Sense::click())
                        .on_hover_text("Clique para fechar");
                    if r.clicked() {
                        dismiss = Some(i);
                    }
                    ui.add_space(4.0);
                }
            });
        if let Some(i) = dismiss {
            self.toasts.remove(i);
        }
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn global_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let (close, next, prev, zoom_in, zoom_out, zoom_reset) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::W),
                i.consume_key(Modifiers::CTRL, Key::Tab),
                i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab),
                i.consume_key(Modifiers::COMMAND, Key::Plus) || i.consume_key(Modifiers::COMMAND, Key::Equals),
                i.consume_key(Modifiers::COMMAND, Key::Minus),
                i.consume_key(Modifiers::COMMAND, Key::Num0),
            )
        });
        if close && !self.tabs.is_empty() {
            self.close_tab(self.active);
        }
        if next && !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
        if prev && !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
        if zoom_in || zoom_out || zoom_reset {
            let z = &mut self.config.settings.zoom;
            *z = if zoom_reset { 1.0 } else if zoom_in { (*z + 0.1).min(2.5) } else { (*z - 0.1).max(0.6) };
            self.save_config();
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        // Desenvolvimento: com MM_SHOT_DIR definido, F12 salva um screenshot numerado.
        if let Some(dir) = std::env::var_os("MM_SHOT_DIR")
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F12)) {
                self.shot_counter += 1;
                let path = std::path::Path::new(&dir).join(format!("shot-{:02}.png", self.shot_counter));
                ctx.request_screenshot(move |img| {
                    if let Err(e) = save_png(&path.to_string_lossy(), &img) {
                        eprintln!("screenshot: {e}");
                    }
                });
            }
        let Some((path, at)) = self.screenshot.clone() else { return };
        ctx.request_repaint_after(Duration::from_millis(50));
        if Instant::now() < at {
            return;
        }
        self.screenshot = None;
        let ctx2 = ctx.clone();
        ctx.request_screenshot(move |img| {
            if let Err(e) = save_png(&path, &img) {
                eprintln!("screenshot: {e}");
            }
            if std::env::var("MM_SCREENSHOT_EXIT").is_ok() {
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ctx.request_repaint();
    }
}

fn save_png(path: &str, img: &egui::ColorImage) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header()?;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
    w.write_image_data(&bytes)?;
    Ok(())
}

/// Esconde a senha de uma URI para exibição.
pub fn redact_uri(uri: &str) -> String {
    if let Some((scheme, rest)) = uri.split_once("://")
        && let Some((creds, host)) = rest.split_once('@')
            && let Some((user, _)) = creds.split_once(':') {
                return format!("{scheme}://{user}:****@{host}");
            }
    uri.to_string()
}

fn status_dot(ui: &mut Ui, filled: bool, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    if filled {
        ui.painter().circle_filled(rect.center(), 4.5, color);
    } else {
        ui.painter().circle_stroke(rect.center(), 4.0, egui::Stroke::new(1.5, color));
    }
}

/// Linha da árvore com indentação e seta opcional. A linha inteira é clicável.
fn tree_row(ui: &mut Ui, indent: usize, expanded: Option<bool>, add: impl FnOnce(&mut Ui)) -> egui::Response {
    tree_row_impl(ui, indent, expanded, false, add)
}

fn tree_row_selected(ui: &mut Ui, indent: usize, selected: bool, add: impl FnOnce(&mut Ui)) -> egui::Response {
    tree_row_impl(ui, indent, None, selected, add)
}

fn tree_row_impl(ui: &mut Ui, indent: usize, expanded: Option<bool>, selected: bool, add: impl FnOnce(&mut Ui)) -> egui::Response {
    let height = 24.0;
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 4, ui.visuals().selection.bg_fill.gamma_multiply(0.6));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(4.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.add_space(indent as f32 * 16.0);
    match expanded {
        Some(true) => {
            child.label(RichText::new("⏷").small().weak());
        }
        Some(false) => {
            child.label(RichText::new("⏵").small().weak());
        }
        None => {
            child.add_space(14.0);
        }
    }
    add(&mut child);
    resp
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.process_replies();
        if (self.applied_zoom - self.config.settings.zoom).abs() > 0.001 {
            ctx.set_zoom_factor(self.config.settings.zoom);
            self.applied_zoom = self.config.settings.zoom;
        }
        self.global_shortcuts(&ctx);

        egui::Panel::top("top")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(270.0)
            .size_range(180.0..=600.0)
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| {
            if self.tabs.is_empty() {
                self.welcome(ui);
            } else {
                self.tab_bar(ui);
                ui.add_space(2.0);
                let idx = self.active.min(self.tabs.len() - 1);
                self.active = idx;
                self.tab_content(ui, idx);
            }
        });

        self.show_dialogs(&ctx);
        self.show_toasts(&ctx);
        self.handle_screenshot(&ctx);
        if self.script.is_some() {
            ctx.request_repaint_after(Duration::from_millis(15));
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(script) = &mut self.script {
            script.inject(ctx, raw_input);
            if script.quit {
                self.script = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.sync_column_prefs();
        let _ = self.config.save();
    }
}
