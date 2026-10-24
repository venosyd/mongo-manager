//! Operações assíncronas no MongoDB. Todas retornam `Result<_, String>` com
//! mensagens prontas para exibir ao usuário.

use crate::shell;
use bson::{Bson, Document, doc};
use futures_util::TryStreamExt;
use mongodb::{
    Client,
    error::{Error as MError, ErrorKind},
    options::{ClientOptions, Hint, UpdateModifications},
    results::CollectionType,
};
use std::collections::HashMap;
use std::io::Write;
use std::time::{Duration, Instant};

pub type Res<T> = Result<T, String>;

pub fn err_str(e: MError) -> String {
    match e.kind.as_ref() {
        ErrorKind::Command(c) => format!("{} (código {} {})", c.message, c.code, c.code_name),
        ErrorKind::Write(mongodb::error::WriteFailure::WriteError(w)) => {
            format!("{} (código {})", w.message, w.code)
        }
        ErrorKind::ServerSelection { message, .. } => {
            format!("Não foi possível conectar ao servidor: {message}")
        }
        _ => e.to_string(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ns {
    pub db: String,
    pub coll: String,
}

impl Ns {
    pub fn new(db: impl Into<String>, coll: impl Into<String>) -> Self {
        Self {
            db: db.into(),
            coll: coll.into(),
        }
    }
    fn collection(&self, client: &Client) -> mongodb::Collection<Document> {
        client.database(&self.db).collection(&self.coll)
    }
}

impl std::fmt::Display for Ns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.db, self.coll)
    }
}

#[derive(Clone, Debug)]
pub struct DbInfo {
    pub name: String,
    pub size_on_disk: u64,
}

#[derive(Clone, Debug)]
pub struct CollInfo {
    pub name: String,
    pub kind: CollKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollKind {
    Collection,
    View,
    Timeseries,
}

pub struct Connected {
    pub client: Client,
    pub dbs: Vec<DbInfo>,
    pub default_db: Option<String>,
    pub version: String,
}

pub async fn connect(uri: String) -> Res<Connected> {
    let mut opts = ClientOptions::parse(&uri).await.map_err(err_str)?;
    if opts.server_selection_timeout.is_none() {
        opts.server_selection_timeout = Some(Duration::from_secs(8));
    }
    if opts.connect_timeout.is_none() {
        opts.connect_timeout = Some(Duration::from_secs(8));
    }
    opts.app_name.get_or_insert_with(|| "mongo-manager".into());
    let default_db = opts.default_database.clone();
    let client = Client::with_options(opts).map_err(err_str)?;
    let info = client
        .database("admin")
        .run_command(doc! { "buildInfo": 1 })
        .await
        .map_err(err_str)?;
    let version = info.get_str("version").unwrap_or("?").to_string();
    let dbs = list_databases(client.clone(), default_db.clone()).await?;
    Ok(Connected {
        client,
        dbs,
        default_db,
        version,
    })
}

pub async fn list_databases(client: Client, default_db: Option<String>) -> Res<Vec<DbInfo>> {
    match client.list_databases().authorized_databases(true).await {
        Ok(list) => {
            let mut dbs: Vec<DbInfo> = list
                .into_iter()
                .map(|d| DbInfo {
                    name: d.name,
                    size_on_disk: d.size_on_disk,
                })
                .collect();
            if let Some(def) = default_db
                && !dbs.iter().any(|d| d.name == def) {
                    dbs.push(DbInfo {
                        name: def,
                        size_on_disk: 0,
                    });
                }
            dbs.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(dbs)
        }
        // Sem permissão de listDatabases: mostra ao menos o banco da URI.
        Err(e) => match default_db {
            Some(def) => Ok(vec![DbInfo {
                name: def,
                size_on_disk: 0,
            }]),
            None => Err(err_str(e)),
        },
    }
}

pub async fn list_collections(client: Client, db: String) -> Res<Vec<CollInfo>> {
    let cursor = client.database(&db).list_collections().await.map_err(err_str)?;
    let specs: Vec<_> = cursor.try_collect().await.map_err(err_str)?;
    let mut out: Vec<CollInfo> = specs
        .into_iter()
        .map(|s| CollInfo {
            kind: match s.collection_type {
                CollectionType::View => CollKind::View,
                CollectionType::Timeseries => CollKind::Timeseries,
                _ => CollKind::Collection,
            },
            name: s.name,
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    Ok(out)
}

#[derive(Clone, Debug, Default)]
pub struct FindSpec {
    pub filter: Document,
    pub projection: Option<Document>,
    pub sort: Option<Document>,
    pub hint: Option<Bson>,
    pub skip: u64,
    pub limit: u64,
    pub max_time_ms: u64,
}

/// Inverte a direção de um sort (`1` ↔ `-1`). Retorna `None` se algum campo não puder ser invertido
/// (ex.: `{$meta: "textScore"}`).
fn invert_sort(sort: &Document) -> Option<Document> {
    let mut out = Document::new();
    for (k, v) in sort {
        let dir = match v {
            Bson::Int32(n) => *n as f64,
            Bson::Int64(n) => *n as f64,
            Bson::Double(n) => *n,
            _ => return None,
        };
        out.insert(k.clone(), if dir < 0.0 { 1 } else { -1 });
    }
    Some(out)
}

fn to_hint(b: &Bson) -> Option<Hint> {
    match b {
        Bson::String(s) => Some(Hint::Name(s.clone())),
        Bson::Document(d) => Some(Hint::Keys(d.clone())),
        _ => None,
    }
}

pub struct Page {
    pub docs: Vec<Document>,
    pub elapsed_ms: u128,
}

/// Busca uma página. Se a página estiver na metade final do resultado e o total for conhecido,
/// lê "de trás para frente" com o sort invertido — assim pular para a última página é tão rápido
/// quanto abrir a primeira.
pub async fn find_page(client: Client, ns: Ns, spec: FindSpec, total: Option<u64>) -> Res<Page> {
    let start = Instant::now();
    let coll = ns.collection(&client);
    let max_time = Duration::from_millis(spec.max_time_ms.max(1));

    let reverse = total.and_then(|total| {
        if spec.skip == 0 || spec.skip < total / 2 || spec.skip >= total || spec.hint.is_some() {
            return None;
        }
        let rev_sort = match &spec.sort {
            Some(s) if !s.is_empty() => invert_sort(s)?,
            // Sem sort só é seguro usar $natural quando não há filtro (ordem natural).
            _ if spec.filter.is_empty() => doc! { "$natural": -1 },
            _ => return None,
        };
        let end = (spec.skip + spec.limit).min(total);
        Some((rev_sort, total - end, end - spec.skip))
    });

    let docs: Vec<Document> = if let Some((rev_sort, rev_skip, rev_limit)) = reverse {
        let mut find = coll
            .find(spec.filter.clone())
            .sort(rev_sort)
            .skip(rev_skip)
            .limit(rev_limit as i64)
            .max_time(max_time);
        if let Some(p) = spec.projection.clone() {
            find = find.projection(p);
        }
        let mut docs: Vec<Document> = find.await.map_err(err_str)?.try_collect().await.map_err(err_str)?;
        docs.reverse();
        docs
    } else {
        let mut find = coll
            .find(spec.filter.clone())
            .skip(spec.skip)
            .limit(spec.limit as i64)
            .max_time(max_time);
        if let Some(s) = spec.sort.clone().filter(|s| !s.is_empty()) {
            find = find.sort(s);
        }
        if let Some(p) = spec.projection.clone() {
            find = find.projection(p);
        }
        if let Some(h) = spec.hint.as_ref().and_then(to_hint) {
            find = find.hint(h);
        }
        find.await.map_err(err_str)?.try_collect().await.map_err(err_str)?
    };
    Ok(Page {
        docs,
        elapsed_ms: start.elapsed().as_millis(),
    })
}

/// Conta documentos. Sem filtro usa a estimativa (instantânea) dos metadados.
pub async fn count(client: Client, ns: Ns, filter: Document, max_time_ms: u64) -> Res<u64> {
    let coll = ns.collection(&client);
    if filter.is_empty()
        && let Ok(n) = coll.estimated_document_count().await {
            return Ok(n);
        }
    coll.count_documents(filter)
        .max_time(Duration::from_millis(max_time_ms.max(1)))
        .await
        .map_err(err_str)
}

pub fn pipeline_writes(pipeline: &[Document]) -> bool {
    pipeline
        .last()
        .is_some_and(|s| s.contains_key("$out") || s.contains_key("$merge"))
}

pub async fn aggregate_page(
    client: Client,
    ns: Ns,
    mut pipeline: Vec<Document>,
    skip: u64,
    limit: u64,
    max_time_ms: u64,
) -> Res<Page> {
    let start = Instant::now();
    let writes = pipeline_writes(&pipeline);
    if !writes {
        if skip > 0 {
            pipeline.push(doc! { "$skip": skip as i64 });
        }
        pipeline.push(doc! { "$limit": limit as i64 });
    }
    let cursor = ns
        .collection(&client)
        .aggregate(pipeline)
        .allow_disk_use(true)
        .max_time(Duration::from_millis(max_time_ms.max(1)))
        .await
        .map_err(err_str)?;
    let docs: Vec<Document> = cursor.try_collect().await.map_err(err_str)?;
    Ok(Page {
        docs,
        elapsed_ms: start.elapsed().as_millis(),
    })
}

pub async fn aggregate_count(client: Client, ns: Ns, mut pipeline: Vec<Document>, max_time_ms: u64) -> Res<u64> {
    if pipeline_writes(&pipeline) {
        return Err("pipeline com $out/$merge".into());
    }
    pipeline.push(doc! { "$count": "n" });
    let cursor = ns
        .collection(&client)
        .aggregate(pipeline)
        .allow_disk_use(true)
        .max_time(Duration::from_millis(max_time_ms.max(1)))
        .await
        .map_err(err_str)?;
    let docs: Vec<Document> = cursor.try_collect().await.map_err(err_str)?;
    Ok(docs
        .first()
        .and_then(|d| d.get("n"))
        .and_then(|v| match v {
            Bson::Int32(n) => Some(*n as u64),
            Bson::Int64(n) => Some(*n as u64),
            Bson::Double(n) => Some(*n as u64),
            _ => None,
        })
        .unwrap_or(0))
}

#[derive(Clone, Debug, Default)]
pub struct FieldInfo {
    pub path: String,
    pub count: usize,
    pub types: Vec<(String, usize)>,
}

fn collect_paths(doc: &Document, prefix: &str, depth: usize, acc: &mut Vec<FieldInfo>, index: &mut HashMap<String, usize>) {
    for (k, v) in doc {
        let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        let i = *index.entry(path.clone()).or_insert_with(|| {
            acc.push(FieldInfo {
                path: path.clone(),
                ..Default::default()
            });
            acc.len() - 1
        });
        let t = shell::type_name(v).to_string();
        let info = &mut acc[i];
        info.count += 1;
        match info.types.iter_mut().find(|(n, _)| *n == t) {
            Some((_, c)) => *c += 1,
            None => info.types.push((t, 1)),
        }
        if depth >= 6 {
            continue;
        }
        match v {
            Bson::Document(sub) => collect_paths(sub, &path, depth + 1, acc, index),
            Bson::Array(arr) => {
                for item in arr.iter().take(20) {
                    if let Bson::Document(sub) = item {
                        collect_paths(sub, &path, depth + 1, acc, index);
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn analyze_fields(docs: &[Document]) -> Vec<FieldInfo> {
    let mut acc = Vec::new();
    let mut index = HashMap::new();
    for d in docs {
        collect_paths(d, "", 0, &mut acc, &mut index);
    }
    for f in &mut acc {
        f.types.sort_by_key(|t| std::cmp::Reverse(t.1));
    }
    acc
}

/// Amostra documentos para descobrir os campos (usado no autocomplete e na aba Esquema).
pub async fn sample_fields(client: Client, ns: Ns, size: u32) -> Res<(Vec<FieldInfo>, usize)> {
    let coll = ns.collection(&client);
    let docs: Vec<Document> = match coll
        .aggregate(vec![doc! { "$sample": { "size": size as i64 } }])
        .max_time(Duration::from_secs(20))
        .await
    {
        Ok(c) => c.try_collect().await.map_err(err_str)?,
        Err(_) => coll
            .find(doc! {})
            .limit(size as i64)
            .max_time(Duration::from_secs(20))
            .await
            .map_err(err_str)?
            .try_collect()
            .await
            .map_err(err_str)?,
    };
    Ok((analyze_fields(&docs), docs.len()))
}

pub async fn explain(client: Client, ns: Ns, inner: Document) -> Res<Document> {
    let db = client.database(&ns.db);
    match db
        .run_command(doc! { "explain": inner.clone(), "verbosity": "executionStats" })
        .await
    {
        Ok(d) => Ok(d),
        Err(_) => db
            .run_command(doc! { "explain": inner, "verbosity": "queryPlanner" })
            .await
            .map_err(err_str),
    }
}

pub fn explain_find_cmd(ns: &Ns, spec: &FindSpec) -> Document {
    let mut cmd = doc! { "find": &ns.coll, "filter": spec.filter.clone() };
    if let Some(s) = spec.sort.clone().filter(|s| !s.is_empty()) {
        cmd.insert("sort", s);
    }
    if let Some(p) = spec.projection.clone() {
        cmd.insert("projection", p);
    }
    if let Some(h) = spec.hint.clone() {
        cmd.insert("hint", h);
    }
    if spec.skip > 0 {
        cmd.insert("skip", spec.skip as i64);
    }
    cmd.insert("limit", spec.limit as i64);
    cmd
}

pub fn explain_agg_cmd(ns: &Ns, pipeline: &[Document]) -> Document {
    doc! { "aggregate": &ns.coll, "pipeline": pipeline.to_vec(), "cursor": {} }
}

pub struct IndexList {
    pub indexes: Vec<Document>,
    pub sizes: HashMap<String, i64>,
    pub usage: HashMap<String, i64>,
}

fn as_i64(b: &Bson) -> Option<i64> {
    match b {
        Bson::Int32(n) => Some(*n as i64),
        Bson::Int64(n) => Some(*n),
        Bson::Double(n) => Some(*n as i64),
        _ => None,
    }
}

pub async fn list_indexes(client: Client, ns: Ns) -> Res<IndexList> {
    let db = client.database(&ns.db);
    let res = db
        .run_command(doc! { "listIndexes": &ns.coll })
        .await
        .map_err(err_str)?;
    let indexes: Vec<Document> = res
        .get_document("cursor")
        .ok()
        .and_then(|c| c.get_array("firstBatch").ok())
        .map(|a| a.iter().filter_map(|b| b.as_document().cloned()).collect())
        .unwrap_or_default();
    let mut sizes = HashMap::new();
    if let Ok(stats) = db.run_command(doc! { "collStats": &ns.coll }).await
        && let Ok(s) = stats.get_document("indexSizes") {
            for (k, v) in s {
                if let Some(n) = as_i64(v) {
                    sizes.insert(k.clone(), n);
                }
            }
        }
    let mut usage = HashMap::new();
    if let Ok(cursor) = ns
        .collection(&client)
        .aggregate(vec![doc! { "$indexStats": {} }])
        .await
    {
        let stats: Vec<Document> = cursor.try_collect().await.unwrap_or_default();
        for s in stats {
            if let (Ok(name), Ok(acc)) = (s.get_str("name"), s.get_document("accesses"))
                && let Some(ops) = acc.get("ops").and_then(as_i64) {
                    usage.insert(name.to_string(), ops);
                }
        }
    }
    Ok(IndexList { indexes, sizes, usage })
}

pub async fn create_index(client: Client, ns: Ns, index: Document) -> Res<String> {
    let res = client
        .database(&ns.db)
        .run_command(doc! { "createIndexes": &ns.coll, "indexes": [index] })
        .await
        .map_err(err_str)?;
    Ok(res.get_str("note").unwrap_or("Índice criado").to_string())
}

pub async fn drop_index(client: Client, ns: Ns, name: String) -> Res<()> {
    client
        .database(&ns.db)
        .run_command(doc! { "dropIndexes": &ns.coll, "index": name })
        .await
        .map(|_| ())
        .map_err(err_str)
}

pub async fn coll_stats(client: Client, ns: Ns) -> Res<Document> {
    client
        .database(&ns.db)
        .run_command(doc! { "collStats": &ns.coll })
        .await
        .map_err(err_str)
}

pub async fn insert_docs(client: Client, ns: Ns, docs: Vec<Document>) -> Res<usize> {
    if docs.is_empty() {
        return Ok(0);
    }
    let n = docs.len();
    ns.collection(&client).insert_many(docs).await.map_err(err_str)?;
    Ok(n)
}

pub async fn replace_doc(client: Client, ns: Ns, id: Bson, doc: Document) -> Res<u64> {
    let r = ns
        .collection(&client)
        .replace_one(doc! { "_id": id }, doc)
        .await
        .map_err(err_str)?;
    if r.matched_count == 0 {
        return Err("Documento não encontrado (pode ter sido removido ou o _id mudou).".into());
    }
    Ok(r.modified_count)
}

pub async fn update_one_by_id(client: Client, ns: Ns, id: Bson, update: Document) -> Res<u64> {
    let r = ns
        .collection(&client)
        .update_one(doc! { "_id": id }, update)
        .await
        .map_err(err_str)?;
    if r.matched_count == 0 {
        return Err("Documento não encontrado.".into());
    }
    Ok(r.modified_count)
}

pub async fn delete_by_id(client: Client, ns: Ns, id: Bson) -> Res<u64> {
    ns.collection(&client)
        .delete_one(doc! { "_id": id })
        .await
        .map(|r| r.deleted_count)
        .map_err(err_str)
}

pub async fn update_many(client: Client, ns: Ns, filter: Document, update: Bson) -> Res<(u64, u64)> {
    let modifications = match update {
        Bson::Document(d) => UpdateModifications::Document(d),
        Bson::Array(a) => UpdateModifications::Pipeline(a.into_iter().filter_map(|b| b.as_document().cloned()).collect()),
        _ => return Err("A atualização deve ser um documento ou um pipeline".into()),
    };
    let r = ns
        .collection(&client)
        .update_many(filter, modifications)
        .await
        .map_err(err_str)?;
    Ok((r.matched_count, r.modified_count))
}

pub async fn delete_many(client: Client, ns: Ns, filter: Document) -> Res<u64> {
    ns.collection(&client)
        .delete_many(filter)
        .await
        .map(|r| r.deleted_count)
        .map_err(err_str)
}

pub async fn create_collection(client: Client, ns: Ns, options: Document) -> Res<()> {
    let mut cmd = doc! { "create": &ns.coll };
    cmd.extend(options);
    client
        .database(&ns.db)
        .run_command(cmd)
        .await
        .map(|_| ())
        .map_err(err_str)
}

pub async fn drop_collection(client: Client, ns: Ns) -> Res<()> {
    ns.collection(&client).drop().await.map_err(err_str)
}

pub async fn drop_database(client: Client, db: String) -> Res<()> {
    client.database(&db).drop().await.map_err(err_str)
}

pub async fn rename_collection(client: Client, ns: Ns, new_name: String) -> Res<()> {
    client
        .database("admin")
        .run_command(doc! {
            "renameCollection": ns.to_string(),
            "to": format!("{}.{}", ns.db, new_name),
        })
        .await
        .map(|_| ())
        .map_err(err_str)
}

pub async fn run_command(client: Client, db: String, cmd: Document) -> Res<Document> {
    client.database(&db).run_command(cmd).await.map_err(err_str)
}

// ---------------------------------------------------------------------------
// Exportação / importação

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    JsonArray,
    JsonLines,
    Csv,
}

pub enum ExportSource {
    Find(Box<FindSpec>),
    Aggregate(Vec<Document>),
}

fn flatten_into(prefix: &str, doc: &Document, out: &mut Vec<(String, Bson)>) {
    for (k, v) in doc {
        let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match v {
            Bson::Document(sub) if !sub.is_empty() => flatten_into(&path, sub, out),
            other => out.push((path, other.clone())),
        }
    }
}

pub fn flatten(doc: &Document) -> Vec<(String, Bson)> {
    let mut out = Vec::new();
    flatten_into("", doc, &mut out);
    out
}

pub fn csv_value(b: &Bson) -> String {
    match b {
        Bson::String(s) => s.clone(),
        Bson::ObjectId(o) => o.to_hex(),
        Bson::DateTime(d) => shell::format_date(d),
        Bson::Int32(n) => n.to_string(),
        Bson::Int64(n) => n.to_string(),
        Bson::Double(n) => n.to_string(),
        Bson::Decimal128(d) => d.to_string(),
        Bson::Boolean(b) => b.to_string(),
        Bson::Null | Bson::Undefined => String::new(),
        other => other.clone().into_relaxed_extjson().to_string(),
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub async fn export(client: Client, ns: Ns, source: ExportSource, path: std::path::PathBuf, format: ExportFormat) -> Res<u64> {
    let coll = ns.collection(&client);
    let mut cursor = match source {
        ExportSource::Find(spec) => {
            let spec = *spec;
            let mut find = coll.find(spec.filter);
            if let Some(s) = spec.sort.filter(|s| !s.is_empty()) {
                find = find.sort(s);
            }
            if let Some(p) = spec.projection {
                find = find.projection(p);
            }
            if spec.limit > 0 {
                find = find.limit(spec.limit as i64);
            }
            if spec.skip > 0 {
                find = find.skip(spec.skip);
            }
            find.await.map_err(err_str)?
        }
        ExportSource::Aggregate(p) => coll.aggregate(p).allow_disk_use(true).await.map_err(err_str)?,
    };
    let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut w = std::io::BufWriter::new(file);
    let io = |e: std::io::Error| e.to_string();
    let mut n = 0u64;
    match format {
        ExportFormat::JsonArray | ExportFormat::JsonLines => {
            let array = format == ExportFormat::JsonArray;
            if array {
                w.write_all(b"[\n").map_err(io)?;
            }
            while let Some(d) = cursor.try_next().await.map_err(err_str)? {
                if array && n > 0 {
                    w.write_all(b",\n").map_err(io)?;
                }
                let json = Bson::Document(d).into_relaxed_extjson().to_string();
                w.write_all(json.as_bytes()).map_err(io)?;
                if !array {
                    w.write_all(b"\n").map_err(io)?;
                }
                n += 1;
            }
            if array {
                w.write_all(b"\n]\n").map_err(io)?;
            }
        }
        ExportFormat::Csv => {
            // As colunas são definidas pelos primeiros 1000 documentos.
            let mut head = Vec::new();
            while head.len() < 1000 {
                match cursor.try_next().await.map_err(err_str)? {
                    Some(d) => head.push(d),
                    None => break,
                }
            }
            let mut columns: Vec<String> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for d in &head {
                for (k, _) in flatten(d) {
                    if seen.insert(k.clone()) {
                        columns.push(k);
                    }
                }
            }
            let header: Vec<String> = columns.iter().map(|c| csv_escape(c)).collect();
            writeln!(w, "{}", header.join(",")).map_err(io)?;
            let write_row = |d: &Document, w: &mut std::io::BufWriter<std::fs::File>| -> std::io::Result<()> {
                let flat: HashMap<String, Bson> = flatten(d).into_iter().collect();
                let row: Vec<String> = columns
                    .iter()
                    .map(|c| flat.get(c).map(|v| csv_escape(&csv_value(v))).unwrap_or_default())
                    .collect();
                writeln!(w, "{}", row.join(","))
            };
            for d in &head {
                write_row(d, &mut w).map_err(io)?;
                n += 1;
            }
            while let Some(d) = cursor.try_next().await.map_err(err_str)? {
                write_row(&d, &mut w).map_err(io)?;
                n += 1;
            }
        }
    }
    w.flush().map_err(io)?;
    Ok(n)
}

/// Lê um CSV (RFC 4180) em linhas de campos.
pub fn parse_csv(src: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => in_quotes = false,
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' => in_quotes = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r| !(r.len() == 1 && r[0].is_empty()));
    rows
}

fn infer_csv_value(s: &str) -> Option<Bson> {
    if s.is_empty() {
        return None;
    }
    if let Ok(n) = s.parse::<i64>() {
        return Some(if n >= i32::MIN as i64 && n <= i32::MAX as i64 { Bson::Int32(n as i32) } else { Bson::Int64(n) });
    }
    if let Ok(f) = s.parse::<f64>()
        && s.chars().any(|c| c.is_ascii_digit()) && !s.contains(char::is_alphabetic) {
            return Some(Bson::Double(f));
        }
    Some(match s {
        "true" => Bson::Boolean(true),
        "false" => Bson::Boolean(false),
        "null" => Bson::Null,
        _ => Bson::String(s.to_string()),
    })
}

/// Define `a.b.c = value` criando os subdocumentos necessários.
pub fn set_path(doc: &mut Document, path: &str, value: Bson) {
    match path.split_once('.') {
        None => {
            doc.insert(path, value);
        }
        Some((head, rest)) => {
            if !matches!(doc.get(head), Some(Bson::Document(_))) {
                doc.insert(head, Document::new());
            }
            if let Some(Bson::Document(sub)) = doc.get_mut(head) {
                set_path(sub, rest, value);
            }
        }
    }
}

pub fn csv_to_docs(src: &str) -> Res<Vec<Document>> {
    let rows = parse_csv(src);
    let Some((header, data)) = rows.split_first() else {
        return Ok(Vec::new());
    };
    let mut docs = Vec::with_capacity(data.len());
    for row in data {
        let mut d = Document::new();
        for (col, val) in header.iter().zip(row.iter()) {
            if let Some(v) = infer_csv_value(val) {
                if col == "_id"
                    && let Ok(oid) = bson::oid::ObjectId::parse_str(val) {
                        d.insert("_id", oid);
                        continue;
                    }
                set_path(&mut d, col, v);
            }
        }
        docs.push(d);
    }
    Ok(docs)
}

pub async fn import(client: Client, ns: Ns, path: std::path::PathBuf, csv: bool) -> Res<u64> {
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let docs = if csv {
        csv_to_docs(&src)?
    } else {
        shell::parse_documents(&src).map_err(|e| e.to_string())?
    };
    let coll = ns.collection(&client);
    let mut n = 0u64;
    for chunk in docs.chunks(1000) {
        coll.insert_many(chunk.to_vec())
            .ordered(false)
            .await
            .map_err(|e| format!("{} (inseridos antes do erro: {n})", err_str(e)))?;
        n += chunk.len() as u64;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert() {
        assert_eq!(invert_sort(&doc! {"a": 1, "b": -1}).unwrap(), doc! {"a": -1, "b": 1});
        assert!(invert_sort(&doc! {"s": {"$meta": "textScore"}}).is_none());
    }

    #[test]
    fn csv_roundtrip() {
        let docs = csv_to_docs("a,b.c,d\n1,\"x,\"\"y\"\"\",true\n2.5,,\n").unwrap();
        assert_eq!(docs[0], doc! {"a": 1, "b": {"c": "x,\"y\""}, "d": true});
        assert_eq!(docs[1], doc! {"a": 2.5});
    }

    #[test]
    fn fields() {
        let f = analyze_fields(&[doc! {"a": 1, "b": {"c": "x"}}, doc! {"a": "s", "arr": [{"z": 1}]}]);
        let paths: Vec<_> = f.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a", "b", "b.c", "arr", "arr.z"]);
        assert_eq!(f[0].count, 2);
    }
}

/// Testes contra um servidor real: `MM_TEST_URI=mongodb://127.0.0.1:27018 cargo test`.
/// Usam apenas o banco temporário `mm_itest`, que é apagado no fim.
#[cfg(test)]
mod integration {
    use super::*;

    fn uri() -> Option<String> {
        std::env::var("MM_TEST_URI").ok()
    }

    #[tokio::test]
    async fn paging_export_import_update() {
        let Some(uri) = uri() else {
            eprintln!("MM_TEST_URI não definido; pulando");
            return;
        };
        let c = connect(uri).await.unwrap();
        let client = c.client;
        let ns = Ns::new("mm_itest", "docs");
        let _ = drop_database(client.clone(), "mm_itest".into()).await;
        let docs: Vec<Document> = (0..1234)
            .map(|i| doc! { "n": i, "g": i % 7, "s": format!("item {i}"), "sub": { "x": i * 2 } })
            .collect();
        insert_docs(client.clone(), ns.clone(), docs).await.unwrap();
        let total = count(client.clone(), ns.clone(), doc! {}, 10_000).await.unwrap();
        assert_eq!(total, 1234);

        // Leitura reversa (metade final) deve devolver exatamente o mesmo que a leitura direta.
        for sort in [None, Some(doc! { "g": -1, "n": 1, "_id": 1 })] {
            for skip in [1200u64, 1000, 650, 1233] {
                let spec = FindSpec {
                    sort: sort.clone(),
                    skip,
                    limit: 50,
                    max_time_ms: 10_000,
                    ..Default::default()
                };
                let fwd = find_page(client.clone(), ns.clone(), spec.clone(), None).await.unwrap().docs;
                let rev = find_page(client.clone(), ns.clone(), spec, Some(total)).await.unwrap().docs;
                assert_eq!(fwd, rev, "sort={sort:?} skip={skip}");
                assert_eq!(fwd.len() as u64, 50.min(total - skip));
            }
        }

        // Exporta (JSON e CSV) e reimporta em outras coleções.
        let dir = std::env::temp_dir().join(format!("mm-itest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (fmt, file, csv) in [
            (ExportFormat::JsonArray, "a.json", false),
            (ExportFormat::JsonLines, "a.jsonl", false),
            (ExportFormat::Csv, "a.csv", true),
        ] {
            let path = dir.join(file);
            let spec = FindSpec { filter: doc! { "g": 3 }, ..Default::default() };
            let n = export(client.clone(), ns.clone(), ExportSource::Find(Box::new(spec)), path.clone(), fmt).await.unwrap();
            let target = Ns::new("mm_itest", file.replace('.', "_"));
            let m = import(client.clone(), target.clone(), path, csv).await.unwrap();
            assert_eq!(n, m, "{file}");
            let back = count(client.clone(), target, doc! { "sub.x": { "$gte": 0 } }, 10_000).await.unwrap();
            assert_eq!(back, n, "{file}: subdocumentos preservados");
        }
        std::fs::remove_dir_all(&dir).ok();

        let (matched, modified) = update_many(client.clone(), ns.clone(), doc! { "g": 0 }, Bson::Document(doc! { "$set": { "flag": true } }))
            .await
            .unwrap();
        assert_eq!((matched, modified), (177, 177));
        let deleted = delete_many(client.clone(), ns.clone(), doc! { "flag": true }).await.unwrap();
        assert_eq!(deleted, 177);

        let (fields, _) = sample_fields(client.clone(), ns.clone(), 100).await.unwrap();
        assert!(fields.iter().any(|f| f.path == "sub.x"));
        let idx = list_indexes(client.clone(), ns.clone()).await.unwrap();
        assert_eq!(idx.indexes.len(), 1);

        drop_database(client, "mm_itest".into()).await.unwrap();
    }
}
