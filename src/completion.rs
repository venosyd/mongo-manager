//! Sugestões de autocomplete sensíveis ao contexto: campos da coleção,
//! operadores de consulta/atualização, estágios e expressões de agregação.

/// Marca a posição do cursor dentro de um snippet.
pub const CARET: char = '‸';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorKind {
    Filter,
    Projection,
    Sort,
    Pipeline,
    Update,
    Document,
    IndexKeys,
    Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Field,
    FieldRef,
    QueryOp,
    UpdateOp,
    Stage,
    Expr,
    Function,
    Value,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Field => "campo",
            ItemKind::FieldRef => "ref. campo",
            ItemKind::QueryOp => "consulta",
            ItemKind::UpdateOp => "atualização",
            ItemKind::Stage => "estágio",
            ItemKind::Expr => "expressão",
            ItemKind::Function => "tipo",
            ItemKind::Value => "valor",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub label: String,
    pub insert: String,
    pub kind: ItemKind,
    pub detail: String,
}

type Entry = (&'static str, &'static str, &'static str);

pub const QUERY_OPS: &[Entry] = &[
    ("$eq", "$eq: ‸", "igual a"),
    ("$ne", "$ne: ‸", "diferente de"),
    ("$gt", "$gt: ‸", "maior que"),
    ("$gte", "$gte: ‸", "maior ou igual a"),
    ("$lt", "$lt: ‸", "menor que"),
    ("$lte", "$lte: ‸", "menor ou igual a"),
    ("$in", "$in: [‸]", "está na lista"),
    ("$nin", "$nin: [‸]", "não está na lista"),
    ("$exists", "$exists: true‸", "campo existe"),
    ("$type", "$type: \"‸\"", "tipo BSON do campo"),
    ("$regex", "$regex: /‸/", "expressão regular"),
    ("$options", "$options: \"‸\"", "opções da regex (i, m, x, s)"),
    ("$all", "$all: [‸]", "array contém todos"),
    ("$elemMatch", "$elemMatch: { ‸ }", "algum elemento do array atende"),
    ("$size", "$size: ‸", "tamanho do array"),
    ("$not", "$not: { ‸ }", "negação"),
    ("$mod", "$mod: [‸, 0]", "resto da divisão"),
    ("$and", "$and: [{ ‸ }]", "todas as condições"),
    ("$or", "$or: [{ ‸ }]", "alguma condição"),
    ("$nor", "$nor: [{ ‸ }]", "nenhuma condição"),
    ("$expr", "$expr: { ‸ }", "expressão de agregação"),
    ("$text", "$text: { $search: \"‸\" }", "busca textual"),
    ("$search", "$search: \"‸\"", "termos da busca textual"),
    ("$where", "$where: \"‸\"", "JavaScript"),
    ("$jsonSchema", "$jsonSchema: { ‸ }", "validação por JSON Schema"),
    ("$geoWithin", "$geoWithin: { ‸ }", "geometria dentro de"),
    ("$geoIntersects", "$geoIntersects: { ‸ }", "geometria intersecta"),
    ("$near", "$near: { ‸ }", "próximo de um ponto"),
    ("$nearSphere", "$nearSphere: { ‸ }", "próximo (esfera)"),
    ("$bitsAllSet", "$bitsAllSet: ‸", "todos os bits ligados"),
    ("$bitsAnySet", "$bitsAnySet: ‸", "algum bit ligado"),
    ("$comment", "$comment: \"‸\"", "comentário na consulta"),
];

pub const UPDATE_OPS: &[Entry] = &[
    ("$set", "$set: { ‸ }", "define valores"),
    ("$unset", "$unset: { ‸: \"\" }", "remove campos"),
    ("$inc", "$inc: { ‸ }", "incrementa"),
    ("$mul", "$mul: { ‸ }", "multiplica"),
    ("$rename", "$rename: { ‸ }", "renomeia campos"),
    ("$min", "$min: { ‸ }", "atualiza se menor"),
    ("$max", "$max: { ‸ }", "atualiza se maior"),
    ("$currentDate", "$currentDate: { ‸: true }", "data atual"),
    ("$setOnInsert", "$setOnInsert: { ‸ }", "define só ao inserir (upsert)"),
    ("$push", "$push: { ‸ }", "adiciona ao array"),
    ("$addToSet", "$addToSet: { ‸ }", "adiciona ao array sem repetir"),
    ("$pull", "$pull: { ‸ }", "remove do array por condição"),
    ("$pullAll", "$pullAll: { ‸ }", "remove valores do array"),
    ("$pop", "$pop: { ‸: 1 }", "remove primeiro (-1) ou último (1)"),
    ("$each", "$each: [‸]", "vários valores em $push/$addToSet"),
    ("$position", "$position: ‸", "posição em $push"),
    ("$slice", "$slice: ‸", "limita o array em $push"),
    ("$sort", "$sort: { ‸ }", "ordena o array em $push"),
    ("$bit", "$bit: { ‸ }", "operação bit a bit"),
];

pub const STAGES: &[Entry] = &[
    ("$match", "$match: { ‸ }", "filtra documentos"),
    ("$project", "$project: { ‸ }", "seleciona/calcula campos"),
    ("$group", "$group: { _id: ‸ }", "agrupa documentos"),
    ("$sort", "$sort: { ‸ }", "ordena"),
    ("$limit", "$limit: ‸", "limita a quantidade"),
    ("$skip", "$skip: ‸", "pula documentos"),
    ("$unwind", "$unwind: \"$‸\"", "desmembra um array"),
    ("$lookup", "$lookup: { from: \"‸\", localField: \"\", foreignField: \"\", as: \"\" }", "junta com outra coleção"),
    ("$addFields", "$addFields: { ‸ }", "adiciona campos"),
    ("$set", "$set: { ‸ }", "adiciona campos (alias de $addFields)"),
    ("$unset", "$unset: \"‸\"", "remove campos"),
    ("$count", "$count: \"‸\"", "conta documentos"),
    ("$facet", "$facet: { ‸ }", "vários pipelines em paralelo"),
    ("$bucket", "$bucket: { groupBy: \"$‸\", boundaries: [], default: \"outros\" }", "agrupa em faixas"),
    ("$bucketAuto", "$bucketAuto: { groupBy: \"$‸\", buckets: 5 }", "faixas automáticas"),
    ("$sortByCount", "$sortByCount: \"$‸\"", "agrupa e ordena por contagem"),
    ("$replaceRoot", "$replaceRoot: { newRoot: \"$‸\" }", "substitui o documento"),
    ("$replaceWith", "$replaceWith: \"$‸\"", "substitui o documento"),
    ("$sample", "$sample: { size: ‸ }", "amostra aleatória"),
    ("$out", "$out: \"‸\"", "grava o resultado em uma coleção"),
    ("$merge", "$merge: { into: \"‸\" }", "mescla o resultado em uma coleção"),
    ("$graphLookup", "$graphLookup: { from: \"‸\", startWith: \"$\", connectFromField: \"\", connectToField: \"\", as: \"\" }", "busca recursiva"),
    ("$unionWith", "$unionWith: \"‸\"", "une com outra coleção"),
    ("$setWindowFields", "$setWindowFields: { partitionBy: \"$‸\", sortBy: {}, output: {} }", "funções de janela"),
    ("$densify", "$densify: { field: \"‸\", range: { step: 1, bounds: \"full\" } }", "preenche lacunas"),
    ("$fill", "$fill: { output: { ‸ } }", "preenche valores nulos"),
    ("$geoNear", "$geoNear: { near: ‸, distanceField: \"dist\" }", "ordena por distância"),
    ("$redact", "$redact: { ‸ }", "restringe conteúdo"),
    ("$indexStats", "$indexStats: {}‸", "estatísticas de uso dos índices"),
    ("$collStats", "$collStats: { storageStats: {} }‸", "estatísticas da coleção"),
    ("$documents", "$documents: [‸]", "documentos literais"),
];

pub const EXPR_OPS: &[Entry] = &[
    ("$sum", "$sum: ‸", "soma"),
    ("$avg", "$avg: ‸", "média"),
    ("$min", "$min: ‸", "mínimo"),
    ("$max", "$max: ‸", "máximo"),
    ("$first", "$first: ‸", "primeiro valor"),
    ("$last", "$last: ‸", "último valor"),
    ("$push", "$push: ‸", "acumula em array"),
    ("$addToSet", "$addToSet: ‸", "acumula valores únicos"),
    ("$count", "$count: {}‸", "contagem"),
    ("$stdDevPop", "$stdDevPop: ‸", "desvio padrão"),
    ("$median", "$median: { input: ‸, method: \"approximate\" }", "mediana"),
    ("$top", "$top: { sortBy: {‸}, output: \"\" }", "maior pelo sort"),
    ("$topN", "$topN: { n: ‸, sortBy: {}, output: \"\" }", "N maiores pelo sort"),
    ("$firstN", "$firstN: { n: ‸, input: \"\" }", "N primeiros"),
    ("$eq", "$eq: [‸]", "igual"),
    ("$ne", "$ne: [‸]", "diferente"),
    ("$gt", "$gt: [‸]", "maior"),
    ("$gte", "$gte: [‸]", "maior ou igual"),
    ("$lt", "$lt: [‸]", "menor"),
    ("$lte", "$lte: [‸]", "menor ou igual"),
    ("$cmp", "$cmp: [‸]", "compara"),
    ("$and", "$and: [‸]", "e lógico"),
    ("$or", "$or: [‸]", "ou lógico"),
    ("$not", "$not: [‸]", "negação"),
    ("$in", "$in: [‸]", "valor está no array"),
    ("$cond", "$cond: { if: ‸, then: , else: }", "condicional"),
    ("$ifNull", "$ifNull: [‸, null]", "valor padrão se nulo"),
    ("$switch", "$switch: { branches: [{ case: ‸, then: }], default: }", "múltiplas condições"),
    ("$add", "$add: [‸]", "soma"),
    ("$subtract", "$subtract: [‸]", "subtração"),
    ("$multiply", "$multiply: [‸]", "multiplicação"),
    ("$divide", "$divide: [‸]", "divisão"),
    ("$mod", "$mod: [‸]", "resto"),
    ("$abs", "$abs: ‸", "valor absoluto"),
    ("$round", "$round: [‸, 2]", "arredonda"),
    ("$trunc", "$trunc: [‸, 0]", "trunca"),
    ("$ceil", "$ceil: ‸", "teto"),
    ("$floor", "$floor: ‸", "piso"),
    ("$pow", "$pow: [‸]", "potência"),
    ("$sqrt", "$sqrt: ‸", "raiz quadrada"),
    ("$concat", "$concat: [‸]", "concatena strings"),
    ("$substrCP", "$substrCP: [‸, 0, 1]", "substring"),
    ("$toLower", "$toLower: ‸", "minúsculas"),
    ("$toUpper", "$toUpper: ‸", "maiúsculas"),
    ("$trim", "$trim: { input: ‸ }", "remove espaços"),
    ("$split", "$split: [‸, \",\"]", "divide string"),
    ("$strLenCP", "$strLenCP: ‸", "tamanho da string"),
    ("$regexMatch", "$regexMatch: { input: ‸, regex: // }", "testa regex"),
    ("$replaceAll", "$replaceAll: { input: ‸, find: \"\", replacement: \"\" }", "substitui texto"),
    ("$dateToString", "$dateToString: { format: \"%Y-%m-%d\", date: ‸ }", "formata data"),
    ("$dateFromString", "$dateFromString: { dateString: ‸ }", "converte string em data"),
    ("$dateAdd", "$dateAdd: { startDate: ‸, unit: \"day\", amount: 1 }", "soma tempo à data"),
    ("$dateDiff", "$dateDiff: { startDate: ‸, endDate: , unit: \"day\" }", "diferença entre datas"),
    ("$dateTrunc", "$dateTrunc: { date: ‸, unit: \"day\" }", "trunca data"),
    ("$year", "$year: ‸", "ano"),
    ("$month", "$month: ‸", "mês"),
    ("$dayOfMonth", "$dayOfMonth: ‸", "dia do mês"),
    ("$dayOfWeek", "$dayOfWeek: ‸", "dia da semana"),
    ("$hour", "$hour: ‸", "hora"),
    ("$minute", "$minute: ‸", "minuto"),
    ("$size", "$size: ‸", "tamanho do array"),
    ("$arrayElemAt", "$arrayElemAt: [‸, 0]", "elemento do array"),
    ("$filter", "$filter: { input: ‸, as: \"item\", cond: {} }", "filtra array"),
    ("$map", "$map: { input: ‸, as: \"item\", in: {} }", "transforma array"),
    ("$reduce", "$reduce: { input: ‸, initialValue: 0, in: {} }", "reduz array"),
    ("$slice", "$slice: [‸, 1]", "parte do array"),
    ("$concatArrays", "$concatArrays: [‸]", "concatena arrays"),
    ("$isArray", "$isArray: ‸", "é array?"),
    ("$indexOfArray", "$indexOfArray: [‸, ]", "posição no array"),
    ("$objectToArray", "$objectToArray: ‸", "objeto ➡ array k/v"),
    ("$arrayToObject", "$arrayToObject: ‸", "array k/v ➡ objeto"),
    ("$mergeObjects", "$mergeObjects: [‸]", "mescla objetos"),
    ("$getField", "$getField: \"‸\"", "lê campo"),
    ("$setUnion", "$setUnion: [‸]", "união de conjuntos"),
    ("$setIntersection", "$setIntersection: [‸]", "interseção"),
    ("$toString", "$toString: ‸", "converte em string"),
    ("$toInt", "$toInt: ‸", "converte em int"),
    ("$toLong", "$toLong: ‸", "converte em long"),
    ("$toDouble", "$toDouble: ‸", "converte em double"),
    ("$toDecimal", "$toDecimal: ‸", "converte em decimal"),
    ("$toDate", "$toDate: ‸", "converte em data"),
    ("$toObjectId", "$toObjectId: ‸", "converte em ObjectId"),
    ("$toBool", "$toBool: ‸", "converte em booleano"),
    ("$convert", "$convert: { input: ‸, to: \"string\" }", "conversão genérica"),
    ("$type", "$type: ‸", "tipo BSON"),
    ("$literal", "$literal: ‸", "valor literal"),
    ("$let", "$let: { vars: { ‸ }, in: }", "variáveis"),
    ("$meta", "$meta: \"textScore\"‸", "metadado"),
];

pub const PROJECTION_OPS: &[Entry] = &[
    ("$slice", "$slice: ‸", "parte do array"),
    ("$elemMatch", "$elemMatch: { ‸ }", "primeiro elemento que atende"),
    ("$meta", "$meta: \"textScore\"‸", "metadado"),
];

pub const FUNCTIONS: &[Entry] = &[
    ("ObjectId", "ObjectId(\"‸\")", "ObjectId"),
    ("ISODate", "ISODate(\"‸\")", "data (ISO 8601)"),
    ("NumberLong", "NumberLong(‸)", "inteiro 64 bits"),
    ("NumberInt", "NumberInt(‸)", "inteiro 32 bits"),
    ("NumberDecimal", "NumberDecimal(\"‸\")", "Decimal128"),
    ("UUID", "UUID(\"‸\")", "UUID"),
    ("Timestamp", "Timestamp({ t: ‸, i: 1 })", "Timestamp"),
    ("RegExp", "RegExp(\"‸\", \"i\")", "regex"),
    ("BinData", "BinData(0, \"‸\")", "binário base64"),
    ("MinKey", "MinKey()‸", "MinKey"),
    ("MaxKey", "MaxKey()‸", "MaxKey"),
    ("true", "true‸", "booleano"),
    ("false", "false‸", "booleano"),
    ("null", "null‸", "nulo"),
];

/// Encontra o início do identificador que termina em `cursor` (índices em caracteres).
pub fn prefix_at(chars: &[char], cursor: usize) -> usize {
    let mut start = cursor.min(chars.len());
    while start > 0 {
        let c = chars[start - 1];
        if c.is_alphanumeric() || c == '_' || c == '$' || c == '.' {
            start -= 1;
        } else {
            break;
        }
    }
    start
}

#[derive(Clone, Debug, Default)]
struct Frame {
    bracket: char,
    key: String,
}

#[derive(Debug, Default)]
struct Context {
    stack: Vec<Frame>,
    key_position: bool,
    in_string: bool,
}

/// Analisa o texto antes do prefixo: pilha de { / [, se estamos numa posição de chave e se dentro de string.
fn analyze(chars: &[char]) -> Context {
    let mut stack: Vec<Frame> = Vec::new();
    let mut last_sig: Option<char> = None;
    let mut last_key = String::new();
    let mut token = String::new();
    let mut i = 0;
    let mut in_string: Option<char> = None;
    let mut string_buf = String::new();
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = in_string {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_string = None;
                token = std::mem::take(&mut string_buf);
                last_sig = Some('"');
            } else {
                string_buf.push(c);
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => {
                in_string = Some(c);
                string_buf.clear();
            }
            '{' | '[' => {
                stack.push(Frame {
                    bracket: c,
                    key: if last_sig == Some(':') { last_key.clone() } else { String::new() },
                });
                last_sig = Some(c);
            }
            '}' | ']' => {
                stack.pop();
                last_sig = Some(c);
            }
            ':' => {
                last_key = std::mem::take(&mut token);
                last_sig = Some(':');
            }
            ',' => last_sig = Some(','),
            c if c.is_whitespace() => {}
            c => {
                if !matches!(last_sig, Some('a')) {
                    token.clear();
                }
                token.push(c);
                last_sig = Some('a');
            }
        }
        i += 1;
    }
    let top_is_object = stack.last().is_some_and(|f| f.bracket == '{');
    Context {
        key_position: top_is_object && matches!(last_sig, Some('{') | Some(',')) && in_string.is_none(),
        in_string: in_string.is_some(),
        stack,
    }
}

fn push_entries(out: &mut Vec<Suggestion>, entries: &[Entry], kind: ItemKind) {
    for (label, snippet, detail) in entries {
        if out.iter().any(|s| s.label == *label) {
            continue;
        }
        out.push(Suggestion {
            label: (*label).into(),
            insert: (*snippet).into(),
            kind,
            detail: (*detail).into(),
        });
    }
}

fn push_fields(out: &mut Vec<Suggestion>, fields: &[String], as_ref: bool, quote: bool) {
    for f in fields {
        let label = if as_ref { format!("${f}") } else { f.clone() };
        let needs_quotes = !as_ref && f.chars().any(|c| !(c.is_alphanumeric() || c == '_' || c == '$' || c == '.'));
        let insert = if quote && (as_ref || needs_quotes) {
            format!("\"{label}\"")
        } else {
            label.clone()
        };
        out.push(Suggestion {
            label,
            insert,
            kind: if as_ref { ItemKind::FieldRef } else { ItemKind::Field },
            detail: String::new(),
        });
    }
}

fn simple_values(out: &mut Vec<Suggestion>, values: &[(&str, &str)]) {
    for (v, d) in values {
        out.push(Suggestion {
            label: (*v).into(),
            insert: (*v).into(),
            kind: ItemKind::Value,
            detail: (*d).into(),
        });
    }
}

/// Gera sugestões para o texto `chars`, com o identificador atual em `start..cursor`.
pub fn suggest(kind: EditorKind, chars: &[char], start: usize, cursor: usize, fields: &[String], forced: bool) -> Vec<Suggestion> {
    let prefix: String = chars[start..cursor.min(chars.len())].iter().collect();
    let ctx = analyze(&chars[..start]);
    let mut out = Vec::new();
    let dollar = prefix.starts_with('$');
    let depth = ctx.stack.len();
    // Pipelines podem ser um array ou um único estágio.
    let pipeline_base = match ctx.stack.first() {
        Some(f) if f.bracket == '[' => 1,
        _ => 0,
    };
    let stage_name = ctx.stack.get(pipeline_base + 1).map(|f| f.key.clone()).unwrap_or_default();
    let quote_fields = !ctx.in_string;

    if ctx.in_string {
        // Dentro de string: nomes de campos ou referências "$campo".
        if dollar || kind == EditorKind::Pipeline {
            push_fields(&mut out, fields, true, false);
        }
        push_fields(&mut out, fields, false, false);
    } else if ctx.key_position {
        match kind {
            EditorKind::Pipeline if depth == pipeline_base + 1 => push_entries(&mut out, STAGES, ItemKind::Stage),
            EditorKind::Pipeline => {
                if stage_name == "$match" && !in_expr(&ctx) {
                    if dollar {
                        push_entries(&mut out, QUERY_OPS, ItemKind::QueryOp);
                    }
                    push_fields(&mut out, fields, false, quote_fields);
                } else {
                    if dollar {
                        push_entries(&mut out, EXPR_OPS, ItemKind::Expr);
                        if stage_name == "$match" {
                            push_entries(&mut out, QUERY_OPS, ItemKind::QueryOp);
                        }
                    }
                    if !dollar || depth <= pipeline_base + 2 {
                        push_fields(&mut out, fields, false, quote_fields);
                    }
                }
            }
            EditorKind::Update => {
                let base = if ctx.stack.first().is_some_and(|f| f.bracket == '[') { 1 } else { 0 };
                if depth == base + 1 {
                    if base == 1 {
                        push_entries(&mut out, STAGES, ItemKind::Stage);
                    } else {
                        push_entries(&mut out, UPDATE_OPS, ItemKind::UpdateOp);
                    }
                } else {
                    if dollar {
                        push_entries(&mut out, UPDATE_OPS, ItemKind::UpdateOp);
                        push_entries(&mut out, EXPR_OPS, ItemKind::Expr);
                    }
                    push_fields(&mut out, fields, false, quote_fields);
                }
            }
            EditorKind::Filter => {
                if in_expr(&ctx) {
                    if dollar {
                        push_entries(&mut out, EXPR_OPS, ItemKind::Expr);
                    }
                } else if dollar {
                    push_entries(&mut out, QUERY_OPS, ItemKind::QueryOp);
                } else {
                    push_fields(&mut out, fields, false, quote_fields);
                }
                if !dollar && depth > 1 && out.is_empty() {
                    push_fields(&mut out, fields, false, quote_fields);
                }
            }
            EditorKind::Projection => {
                if dollar {
                    push_entries(&mut out, PROJECTION_OPS, ItemKind::Expr);
                    push_entries(&mut out, EXPR_OPS, ItemKind::Expr);
                }
                push_fields(&mut out, fields, false, quote_fields);
            }
            EditorKind::Sort | EditorKind::IndexKeys | EditorKind::Document => {
                push_fields(&mut out, fields, false, quote_fields);
            }
            EditorKind::Command => {}
        }
    } else {
        // Posição de valor.
        let expr_ctx = kind == EditorKind::Pipeline && !(stage_name == "$match" && !in_expr(&ctx))
            || (kind == EditorKind::Filter && in_expr(&ctx))
            || (kind == EditorKind::Projection && dollar);
        if dollar && expr_ctx {
            push_fields(&mut out, fields, true, true);
            push_entries(&mut out, EXPR_OPS, ItemKind::Expr);
        }
        match kind {
            EditorKind::Sort => simple_values(&mut out, &[("1", "crescente"), ("-1", "decrescente")]),
            EditorKind::IndexKeys => simple_values(
                &mut out,
                &[
                    ("1", "crescente"),
                    ("-1", "decrescente"),
                    ("\"text\"", "texto"),
                    ("\"2dsphere\"", "geoespacial"),
                    ("\"hashed\"", "hash"),
                ],
            ),
            EditorKind::Projection if !dollar => simple_values(&mut out, &[("1", "incluir"), ("0", "excluir")]),
            _ => {}
        }
        if !dollar {
            push_entries(&mut out, FUNCTIONS, ItemKind::Function);
        }
    }

    rank(out, &prefix, forced)
}

/// Estamos dentro de um `$expr` (onde valem operadores de expressão)?
fn in_expr(ctx: &Context) -> bool {
    ctx.stack.iter().any(|f| f.key == "$expr")
}

fn rank(items: Vec<Suggestion>, prefix: &str, forced: bool) -> Vec<Suggestion> {
    let p = prefix.to_lowercase();
    if p.is_empty() {
        return if forced { items.into_iter().take(200).collect() } else { Vec::new() };
    }
    let mut scored: Vec<(u8, usize, Suggestion)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let l = s.label.to_lowercase();
            if l == p && !forced {
                return None;
            }
            let score = if l.starts_with(&p) {
                0
            } else if l.split('.').any(|seg| seg.starts_with(p.trim_start_matches('$'))) && !p.trim_start_matches('$').is_empty() {
                1
            } else if l.contains(&p) {
                2
            } else {
                return None;
            };
            Some((score, i, s))
        })
        .collect();
    scored.sort_by_key(|(score, i, _)| (*score, *i));
    scored.into_iter().map(|(_, _, s)| s).take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(kind: EditorKind, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let start = prefix_at(&chars, chars.len());
        suggest(kind, &chars, start, chars.len(), &["name".into(), "age".into(), "address.city".into()], false)
            .into_iter()
            .map(|s| s.label)
            .collect()
    }

    #[test]
    fn contexts() {
        assert_eq!(run(EditorKind::Filter, "{ na"), ["name"]);
        assert!(run(EditorKind::Filter, "{ age: { $g").contains(&"$gt".to_string()));
        assert!(run(EditorKind::Filter, "{ $o").contains(&"$or".to_string()));
        assert!(run(EditorKind::Pipeline, "[{ $gr").contains(&"$group".to_string()));
        let v = run(EditorKind::Pipeline, "[{ $group: { _id: $na");
        assert_eq!(v.first().map(String::as_str), Some("$name"));
        assert!(run(EditorKind::Pipeline, "[{ $group: { _id: \"$a\", t: { $su").contains(&"$sum".to_string()));
        assert!(run(EditorKind::Pipeline, "[{ $match: { age: { $i").contains(&"$in".to_string()));
        assert!(run(EditorKind::Update, "{ $se").contains(&"$set".to_string()));
        assert_eq!(run(EditorKind::Update, "{ $set: { ag"), ["age"]);
        assert!(run(EditorKind::Filter, "{ _id: Obj").contains(&"ObjectId".to_string()));
        assert!(run(EditorKind::Filter, "{ ci").contains(&"address.city".to_string()));
        assert!(run(EditorKind::Pipeline, "[{ $unwind: \"$ad").contains(&"$address.city".to_string()));
    }
}
