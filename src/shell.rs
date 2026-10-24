//! Parser e formatador da sintaxe "estilo mongosh".
//!
//! Aceita um superconjunto de JSON: chaves sem aspas, strings com aspas simples,
//! vírgulas sobrando, comentários, regex literal (`/abc/i`), construtores do shell
//! (`ObjectId("..")`, `ISODate("..")`, `NumberLong(..)`, ...) e Extended JSON
//! (`{"$oid": ".."}`, `{"$date": ".."}`, ...).

use base64::Engine;
use bson::{
    Binary, Bson, DateTime, Decimal128, Document, JavaScriptCodeWithScope, Regex, Timestamp,
    oid::ObjectId, spec::BinarySubtype,
};
use std::fmt::{self, Write};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub msg: String,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "linha {}, coluna {}: {}", self.line, self.col, self.msg)
    }
}

impl std::error::Error for ParseError {}

type PResult<T> = Result<T, ParseError>;

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

const MAX_DEPTH: usize = 200;

impl Parser {
    fn new(src: &str) -> Self {
        Self {
            chars: src.chars().collect(),
            pos: 0,
        }
    }

    fn err<T>(&self, msg: impl Into<String>) -> PResult<T> {
        Err(self.err_at(self.pos, msg))
    }

    fn err_at(&self, pos: usize, msg: impl Into<String>) -> ParseError {
        let mut line = 1;
        let mut col = 1;
        for &c in self.chars.iter().take(pos) {
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        ParseError {
            msg: msg.into(),
            line,
            col,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, off: usize) -> Option<char> {
        self.chars.get(self.pos + off).copied()
    }

    fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn skip_ws(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => self.pos += 1,
                Some('/') if self.peek_at(1) == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                Some('/') if self.peek_at(1) == Some('*') => {
                    self.pos += 2;
                    while !self.at_end() && !(self.peek() == Some('*') && self.peek_at(1) == Some('/')) {
                        self.pos += 1;
                    }
                    self.pos = (self.pos + 2).min(self.chars.len());
                }
                _ => break,
            }
        }
    }

    fn expect(&mut self, c: char) -> PResult<()> {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            match self.peek() {
                Some(found) => self.err(format!("esperado '{c}', encontrado '{found}'")),
                None => self.err(format!("esperado '{c}', mas o texto terminou")),
            }
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn is_ident_start(c: char) -> bool {
        c.is_alphabetic() || c == '_' || c == '$'
    }

    fn is_ident_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == '$'
    }

    fn ident(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if Self::is_ident_char(c) {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn value(&mut self, depth: usize) -> PResult<Bson> {
        if depth > MAX_DEPTH {
            return self.err("aninhamento profundo demais");
        }
        self.skip_ws();
        let Some(c) = self.peek() else {
            return self.err("valor esperado, mas o texto terminou");
        };
        match c {
            '{' => self.object(depth).map(convert_extjson),
            '[' => self.array(depth),
            '"' | '\'' => self.string().map(Bson::String),
            '/' => self.regex_literal(),
            '-' | '+' | '.' | '0'..='9' => self.number(),
            c if Self::is_ident_start(c) => self.ident_value(depth),
            other => self.err(format!("caractere inesperado '{other}'")),
        }
    }

    fn object(&mut self, depth: usize) -> PResult<Bson> {
        self.expect('{')?;
        let mut doc = Document::new();
        loop {
            self.skip_ws();
            if self.eat('}') {
                break;
            }
            let key_pos = self.pos;
            let key = self.key()?;
            self.expect(':')?;
            let v = self.value(depth + 1)?;
            if doc.contains_key(&key) {
                return Err(self.err_at(key_pos, format!("chave duplicada '{key}'")));
            }
            doc.insert(key, v);
            self.skip_ws();
            if self.eat(',') {
                continue;
            }
            if self.eat('}') {
                break;
            }
            return match self.peek() {
                Some(c) => self.err(format!("esperado ',' ou '}}', encontrado '{c}'")),
                None => self.err("objeto não foi fechado com '}'"),
            };
        }
        Ok(Bson::Document(doc))
    }

    fn key(&mut self) -> PResult<String> {
        self.skip_ws();
        match self.peek() {
            Some('"') | Some('\'') => self.string(),
            Some(c) if Self::is_ident_char(c) || c == '.' => {
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if Self::is_ident_char(c) || c == '.' || c == '-' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                Ok(self.chars[start..self.pos].iter().collect())
            }
            Some(c) => self.err(format!("nome de campo esperado, encontrado '{c}'")),
            None => self.err("nome de campo esperado, mas o texto terminou"),
        }
    }

    fn array(&mut self, depth: usize) -> PResult<Bson> {
        self.expect('[')?;
        let mut arr = Vec::new();
        loop {
            self.skip_ws();
            if self.eat(']') {
                break;
            }
            arr.push(self.value(depth + 1)?);
            self.skip_ws();
            if self.eat(',') {
                continue;
            }
            if self.eat(']') {
                break;
            }
            return match self.peek() {
                Some(c) => self.err(format!("esperado ',' ou ']', encontrado '{c}'")),
                None => self.err("array não foi fechado com ']'"),
            };
        }
        Ok(Bson::Array(arr))
    }

    fn string(&mut self) -> PResult<String> {
        let quote = self.peek().unwrap();
        let start = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err_at(start, "string não foi fechada"));
            };
            self.pos += 1;
            if c == quote {
                return Ok(out);
            }
            if c != '\\' {
                out.push(c);
                continue;
            }
            let Some(e) = self.peek() else {
                return self.err("escape incompleto");
            };
            self.pos += 1;
            match e {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'v' => out.push('\u{b}'),
                '0' => out.push('\0'),
                '\n' => {}
                'x' => {
                    let code = self.hex_digits(2)?;
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                'u' => {
                    let hi = self.hex_digits(4)?;
                    if (0xD800..0xDC00).contains(&hi)
                        && self.peek() == Some('\\')
                        && self.peek_at(1) == Some('u')
                    {
                        self.pos += 2;
                        let lo = self.hex_digits(4)?;
                        let code = 0x10000 + ((hi - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    } else {
                        out.push(char::from_u32(hi).unwrap_or('\u{fffd}'));
                    }
                }
                other => out.push(other),
            }
        }
    }

    fn hex_digits(&mut self, n: usize) -> PResult<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            match self.peek().and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    v = v * 16 + d;
                    self.pos += 1;
                }
                None => return self.err("dígito hexadecimal esperado"),
            }
        }
        Ok(v)
    }

    fn regex_literal(&mut self) -> PResult<Bson> {
        let start = self.pos;
        self.pos += 1;
        let mut pattern = String::new();
        let mut in_class = false;
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err_at(start, "regex não foi fechada com '/'"));
            };
            self.pos += 1;
            match c {
                '\\' => {
                    let Some(n) = self.peek() else {
                        return self.err("escape incompleto na regex");
                    };
                    self.pos += 1;
                    if n == '/' {
                        pattern.push('/');
                    } else {
                        pattern.push('\\');
                        pattern.push(n);
                    }
                }
                '[' => {
                    in_class = true;
                    pattern.push(c);
                }
                ']' => {
                    in_class = false;
                    pattern.push(c);
                }
                '/' if !in_class => break,
                '\n' => return Err(self.err_at(start, "regex não foi fechada com '/'")),
                _ => pattern.push(c),
            }
        }
        let mut options = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                options.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(Bson::RegularExpression(make_regex(pattern, &options)))
    }

    fn number(&mut self) -> PResult<Bson> {
        let start = self.pos;
        let mut neg = false;
        if matches!(self.peek(), Some('-') | Some('+')) {
            neg = self.peek() == Some('-');
            self.pos += 1;
        }
        // Infinity / NaN com sinal
        if self.peek().is_some_and(|c| c.is_alphabetic()) {
            let id = self.ident();
            return match id.as_str() {
                "Infinity" => Ok(Bson::Double(if neg { f64::NEG_INFINITY } else { f64::INFINITY })),
                "NaN" => Ok(Bson::Double(f64::NAN)),
                _ => Err(self.err_at(start, "número inválido")),
            };
        }
        if self.peek() == Some('0') && matches!(self.peek_at(1), Some('x') | Some('X')) {
            self.pos += 2;
            let hs = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            let hex: String = self.chars[hs..self.pos].iter().collect();
            let v = i64::from_str_radix(&hex, 16).map_err(|_| self.err_at(start, "hexadecimal inválido"))?;
            let v = if neg { -v } else { v };
            return Ok(int_bson(v));
        }
        let mut is_float = false;
        while let Some(c) = self.peek() {
            match c {
                '0'..='9' | '_' => self.pos += 1,
                '.' => {
                    is_float = true;
                    self.pos += 1;
                }
                'e' | 'E' => {
                    is_float = true;
                    self.pos += 1;
                    if matches!(self.peek(), Some('-') | Some('+')) {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
        let text: String = self.chars[start..self.pos].iter().filter(|c| **c != '_').collect();
        if text.is_empty() || text == "-" || text == "+" || text == "." {
            return Err(self.err_at(start, "número inválido"));
        }
        if !is_float
            && let Ok(v) = text.parse::<i64>() {
                return Ok(int_bson(v));
            }
        text.parse::<f64>()
            .map(Bson::Double)
            .map_err(|_| self.err_at(start, format!("número inválido '{text}'")))
    }

    /// Lê argumentos `( v1, v2, ... )` de um construtor.
    fn call_args(&mut self, depth: usize) -> PResult<Vec<Bson>> {
        let mut args = Vec::new();
        if !self.eat('(') {
            return Ok(args);
        }
        loop {
            self.skip_ws();
            if self.eat(')') {
                break;
            }
            args.push(self.value(depth + 1)?);
            self.skip_ws();
            if self.eat(',') {
                continue;
            }
            if self.eat(')') {
                break;
            }
            return self.err("esperado ',' ou ')'");
        }
        Ok(args)
    }

    fn ident_value(&mut self, depth: usize) -> PResult<Bson> {
        let start = self.pos;
        let mut name = self.ident();
        if name == "new" {
            self.skip_ws();
            name = self.ident();
        }
        match name.as_str() {
            "true" => return Ok(Bson::Boolean(true)),
            "false" => return Ok(Bson::Boolean(false)),
            "null" => return Ok(Bson::Null),
            "undefined" => return Ok(Bson::Undefined),
            "NaN" => return Ok(Bson::Double(f64::NAN)),
            "Infinity" => return Ok(Bson::Double(f64::INFINITY)),
            _ => {}
        }
        self.skip_ws();
        if self.peek() != Some('(') {
            return Err(self.err_at(start, format!("valor desconhecido '{name}' (strings precisam de aspas)")));
        }
        let args = self.call_args(depth)?;
        let fail = |p: &Parser, msg: String| Err(p.err_at(start, msg));
        let str_arg = |i: usize| -> Option<String> {
            match args.get(i) {
                Some(Bson::String(s)) => Some(s.clone()),
                _ => None,
            }
        };
        let num_arg = |i: usize| -> Option<f64> {
            match args.get(i) {
                Some(Bson::Int32(v)) => Some(*v as f64),
                Some(Bson::Int64(v)) => Some(*v as f64),
                Some(Bson::Double(v)) => Some(*v),
                Some(Bson::String(s)) => s.trim().parse().ok(),
                _ => None,
            }
        };
        match name.as_str() {
            "ObjectId" | "ObjectID" => match args.first() {
                None => Ok(Bson::ObjectId(ObjectId::new())),
                Some(Bson::String(s)) => ObjectId::parse_str(s.trim())
                    .map(Bson::ObjectId)
                    .or_else(|_| fail(self, format!("ObjectId inválido \"{s}\" (precisa ter 24 caracteres hexadecimais)"))),
                _ => fail(self, "ObjectId espera uma string".into()),
            },
            "ISODate" | "Date" => match args.first() {
                None => Ok(Bson::DateTime(DateTime::now())),
                Some(Bson::String(s)) => parse_date_str(s)
                    .map(Bson::DateTime)
                    .ok_or_else(|| self.err_at(start, format!("data inválida \"{s}\""))),
                Some(Bson::Int32(v)) => Ok(Bson::DateTime(DateTime::from_millis(*v as i64))),
                Some(Bson::Int64(v)) => Ok(Bson::DateTime(DateTime::from_millis(*v))),
                Some(Bson::Double(v)) => Ok(Bson::DateTime(DateTime::from_millis(*v as i64))),
                _ => fail(self, "data espera uma string ISO ou milissegundos".into()),
            },
            "NumberLong" | "Long" | "Int64" => match args.first() {
                None => Ok(Bson::Int64(0)),
                Some(Bson::String(s)) => s
                    .trim()
                    .parse::<i64>()
                    .map(Bson::Int64)
                    .or_else(|_| fail(self, format!("NumberLong inválido \"{s}\""))),
                Some(Bson::Int32(v)) => Ok(Bson::Int64(*v as i64)),
                Some(Bson::Int64(v)) => Ok(Bson::Int64(*v)),
                Some(Bson::Double(v)) if v.fract() == 0.0 => Ok(Bson::Int64(*v as i64)),
                _ => fail(self, "NumberLong espera um inteiro".into()),
            },
            "NumberInt" | "Int32" => match num_arg(0) {
                None if args.is_empty() => Ok(Bson::Int32(0)),
                Some(v) if v.fract() == 0.0 && v >= i32::MIN as f64 && v <= i32::MAX as f64 => {
                    Ok(Bson::Int32(v as i32))
                }
                _ => fail(self, "NumberInt espera um inteiro de 32 bits".into()),
            },
            "Double" | "NumberDouble" => match num_arg(0) {
                Some(v) => Ok(Bson::Double(v)),
                None => fail(self, "Double espera um número".into()),
            },
            "NumberDecimal" | "Decimal128" => {
                let s = match args.first() {
                    Some(Bson::String(s)) => s.clone(),
                    Some(other) => to_shell_compact(other),
                    None => "0".into(),
                };
                s.trim()
                    .parse::<Decimal128>()
                    .map(Bson::Decimal128)
                    .or_else(|_| fail(self, format!("Decimal128 inválido \"{s}\"")))
            }
            "UUID" => match str_arg(0) {
                Some(s) => bson::Uuid::parse_str(s.trim())
                    .map(|u| Bson::Binary(Binary::from_uuid(u)))
                    .or_else(|_| fail(self, format!("UUID inválido \"{s}\""))),
                None if args.is_empty() => Ok(Bson::Binary(Binary::from_uuid(bson::Uuid::new()))),
                None => fail(self, "UUID espera uma string".into()),
            },
            "BinData" => {
                let subtype = num_arg(0).unwrap_or(0.0) as u8;
                let Some(b64) = str_arg(1) else {
                    return fail(self, "BinData espera (subtipo, \"base64\")".into());
                };
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64.trim())
                    .map_err(|_| self.err_at(start, "base64 inválido em BinData"))?;
                Ok(Bson::Binary(Binary {
                    subtype: BinarySubtype::from(subtype),
                    bytes,
                }))
            }
            "HexData" => {
                let subtype = num_arg(0).unwrap_or(0.0) as u8;
                let Some(hex) = str_arg(1) else {
                    return fail(self, "HexData espera (subtipo, \"hex\")".into());
                };
                let bytes = decode_hex(hex.trim()).ok_or_else(|| self.err_at(start, "hex inválido"))?;
                Ok(Bson::Binary(Binary {
                    subtype: BinarySubtype::from(subtype),
                    bytes,
                }))
            }
            "Timestamp" => {
                let (t, i) = match args.first() {
                    Some(Bson::Document(d)) => (
                        d.get("t").and_then(bson_as_f64).unwrap_or(0.0),
                        d.get("i").and_then(bson_as_f64).unwrap_or(0.0),
                    ),
                    _ => (num_arg(0).unwrap_or(0.0), num_arg(1).unwrap_or(0.0)),
                };
                Ok(Bson::Timestamp(Timestamp {
                    time: t as u32,
                    increment: i as u32,
                }))
            }
            "RegExp" => {
                let Some(p) = str_arg(0) else {
                    return fail(self, "RegExp espera uma string".into());
                };
                Ok(Bson::RegularExpression(make_regex(p, &str_arg(1).unwrap_or_default())))
            }
            "MinKey" => Ok(Bson::MinKey),
            "MaxKey" => Ok(Bson::MaxKey),
            "Code" => {
                let code = str_arg(0).unwrap_or_default();
                match args.get(1) {
                    Some(Bson::Document(scope)) => Ok(Bson::JavaScriptCodeWithScope(JavaScriptCodeWithScope {
                        code,
                        scope: scope.clone(),
                    })),
                    _ => Ok(Bson::JavaScriptCode(code)),
                }
            }
            "Symbol" => Ok(Bson::Symbol(str_arg(0).unwrap_or_default())),
            _ => fail(self, format!("função desconhecida '{name}'")),
        }
    }
}

fn int_bson(v: i64) -> Bson {
    if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
        Bson::Int32(v as i32)
    } else {
        Bson::Int64(v)
    }
}

fn bson_as_f64(b: &Bson) -> Option<f64> {
    match b {
        Bson::Int32(v) => Some(*v as f64),
        Bson::Int64(v) => Some(*v as f64),
        Bson::Double(v) => Some(*v),
        Bson::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn make_regex(pattern: String, options: &str) -> Regex {
    let mut opts: Vec<char> = options.chars().collect();
    opts.sort_unstable();
    opts.dedup();
    Regex {
        pattern,
        options: opts.into_iter().collect(),
    }
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Aceita ISO 8601 completo, só a data (`2024-01-31`) ou data e hora sem fuso (UTC).
pub fn parse_date_str(s: &str) -> Option<DateTime> {
    let s = s.trim();
    if let Ok(d) = DateTime::parse_rfc3339_str(s) {
        return Some(d);
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(DateTime::from_millis(d.timestamp_millis()));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"] {
        if let Ok(nd) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(DateTime::from_millis(nd.and_utc().timestamp_millis()));
        }
    }
    if let Ok(nd) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(DateTime::from_millis(nd.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis()));
    }
    None
}

/// Converte objetos Extended JSON (`{"$oid": ...}` etc.) para o tipo BSON correspondente.
fn convert_extjson(value: Bson) -> Bson {
    let Bson::Document(doc) = value else {
        return value;
    };
    let Some(first) = doc.keys().next() else {
        return Bson::Document(doc);
    };
    if !first.starts_with('$') {
        return Bson::Document(doc);
    }
    let converted = match (doc.len(), first.as_str()) {
        (1, "$oid") => doc.get_str("$oid").ok().and_then(|s| ObjectId::parse_str(s).ok()).map(Bson::ObjectId),
        (1, "$date") => match doc.get("$date") {
            Some(Bson::String(s)) => parse_date_str(s).map(Bson::DateTime),
            Some(Bson::Int64(v)) => Some(Bson::DateTime(DateTime::from_millis(*v))),
            Some(Bson::Int32(v)) => Some(Bson::DateTime(DateTime::from_millis(*v as i64))),
            Some(Bson::Double(v)) => Some(Bson::DateTime(DateTime::from_millis(*v as i64))),
            _ => None,
        },
        (1, "$numberLong") => match doc.get("$numberLong") {
            Some(Bson::String(s)) => s.parse().ok().map(Bson::Int64),
            Some(Bson::Int32(v)) => Some(Bson::Int64(*v as i64)),
            Some(Bson::Int64(v)) => Some(Bson::Int64(*v)),
            _ => None,
        },
        (1, "$numberInt") => doc.get("$numberInt").and_then(bson_as_f64).map(|v| Bson::Int32(v as i32)),
        (1, "$numberDouble") => match doc.get("$numberDouble") {
            Some(Bson::String(s)) => match s.as_str() {
                "Infinity" => Some(Bson::Double(f64::INFINITY)),
                "-Infinity" => Some(Bson::Double(f64::NEG_INFINITY)),
                "NaN" => Some(Bson::Double(f64::NAN)),
                s => s.parse().ok().map(Bson::Double),
            },
            Some(b) => bson_as_f64(b).map(Bson::Double),
            None => None,
        },
        (1, "$numberDecimal") => doc
            .get_str("$numberDecimal")
            .ok()
            .and_then(|s| s.parse::<Decimal128>().ok())
            .map(Bson::Decimal128),
        (1, "$uuid") => doc
            .get_str("$uuid")
            .ok()
            .and_then(|s| bson::Uuid::parse_str(s).ok())
            .map(|u| Bson::Binary(Binary::from_uuid(u))),
        (1, "$binary") => doc.get_document("$binary").ok().and_then(|b| {
            let bytes = base64::engine::general_purpose::STANDARD.decode(b.get_str("base64").ok()?).ok()?;
            let st = b.get_str("subType").ok().and_then(|s| u8::from_str_radix(s, 16).ok())?;
            Some(Bson::Binary(Binary {
                subtype: BinarySubtype::from(st),
                bytes,
            }))
        }),
        (2, "$binary") => {
            let bytes = doc.get_str("$binary").ok().and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok());
            let st = doc.get_str("$type").ok().and_then(|s| u8::from_str_radix(s, 16).ok());
            match (bytes, st) {
                (Some(bytes), Some(st)) => Some(Bson::Binary(Binary {
                    subtype: BinarySubtype::from(st),
                    bytes,
                })),
                _ => None,
            }
        }
        (1, "$regularExpression") => doc.get_document("$regularExpression").ok().and_then(|r| {
            Some(Bson::RegularExpression(make_regex(
                r.get_str("pattern").ok()?.to_string(),
                r.get_str("options").unwrap_or(""),
            )))
        }),
        (2, "$regex") if doc.contains_key("$options") => match (doc.get("$regex"), doc.get_str("$options")) {
            (Some(Bson::String(p)), Ok(o)) => Some(Bson::RegularExpression(make_regex(p.clone(), o))),
            _ => None,
        },
        (1, "$timestamp") => doc.get_document("$timestamp").ok().map(|t| {
            Bson::Timestamp(Timestamp {
                time: t.get("t").and_then(bson_as_f64).unwrap_or(0.0) as u32,
                increment: t.get("i").and_then(bson_as_f64).unwrap_or(0.0) as u32,
            })
        }),
        (1, "$minKey") => Some(Bson::MinKey),
        (1, "$maxKey") => Some(Bson::MaxKey),
        (1, "$undefined") => Some(Bson::Undefined),
        (1, "$symbol") => doc.get_str("$symbol").ok().map(|s| Bson::Symbol(s.into())),
        (1, "$code") => doc.get_str("$code").ok().map(|s| Bson::JavaScriptCode(s.into())),
        (2, "$code") => match (doc.get_str("$code"), doc.get_document("$scope")) {
            (Ok(c), Ok(s)) => Some(Bson::JavaScriptCodeWithScope(JavaScriptCodeWithScope {
                code: c.into(),
                scope: s.clone(),
            })),
            _ => None,
        },
        _ => None,
    };
    converted.unwrap_or(Bson::Document(doc))
}

fn parse_all(src: &str) -> PResult<Option<Bson>> {
    let mut p = Parser::new(src);
    p.skip_ws();
    if p.at_end() {
        return Ok(None);
    }
    let v = p.value(0)?;
    p.skip_ws();
    p.eat(';');
    p.skip_ws();
    if !p.at_end() {
        return p.err("texto extra após o fim do valor");
    }
    Ok(Some(v))
}

/// Interpreta qualquer valor.
pub fn parse_value(src: &str) -> PResult<Bson> {
    parse_all(src)?.ok_or(ParseError {
        msg: "valor vazio".into(),
        line: 1,
        col: 1,
    })
}

/// Interpreta um documento; texto vazio vira `{}`.
pub fn parse_document(src: &str) -> PResult<Document> {
    match parse_all(src)? {
        None => Ok(Document::new()),
        Some(Bson::Document(d)) => Ok(d),
        Some(_) => Err(ParseError {
            msg: "era esperado um documento { ... }".into(),
            line: 1,
            col: 1,
        }),
    }
}

/// Interpreta um pipeline de agregação: array de estágios ou um único estágio.
pub fn parse_pipeline(src: &str) -> PResult<Vec<Document>> {
    match parse_all(src)? {
        None => Ok(Vec::new()),
        Some(Bson::Document(d)) => Ok(vec![d]),
        Some(Bson::Array(a)) => a
            .into_iter()
            .enumerate()
            .map(|(i, v)| match v {
                Bson::Document(d) => Ok(d),
                _ => Err(ParseError {
                    msg: format!("o estágio {} não é um documento", i + 1),
                    line: 1,
                    col: 1,
                }),
            })
            .collect(),
        Some(_) => Err(ParseError {
            msg: "era esperado um array de estágios [ {...}, ... ]".into(),
            line: 1,
            col: 1,
        }),
    }
}

/// Interpreta um ou mais documentos: array, documento único ou um documento por linha (JSONL).
pub fn parse_documents(src: &str) -> PResult<Vec<Document>> {
    match parse_all(src) {
        Ok(None) => Ok(Vec::new()),
        Ok(Some(Bson::Document(d))) => Ok(vec![d]),
        Ok(Some(Bson::Array(a))) => a
            .into_iter()
            .enumerate()
            .map(|(i, v)| match v {
                Bson::Document(d) => Ok(d),
                _ => Err(ParseError {
                    msg: format!("o item {} não é um documento", i + 1),
                    line: 1,
                    col: 1,
                }),
            })
            .collect(),
        Ok(Some(_)) => Err(ParseError {
            msg: "era esperado um documento ou array de documentos".into(),
            line: 1,
            col: 1,
        }),
        Err(first_err) => {
            // Tenta JSON Lines / documentos concatenados.
            let mut p = Parser::new(src);
            let mut docs = Vec::new();
            loop {
                p.skip_ws();
                while p.eat(',') {
                    p.skip_ws();
                }
                if p.at_end() {
                    break;
                }
                if p.peek() != Some('{') {
                    return Err(first_err);
                }
                match p.value(0) {
                    Ok(Bson::Document(d)) => docs.push(d),
                    Ok(_) => return Err(first_err),
                    Err(e) => return Err(e),
                }
            }
            if docs.len() > 1 { Ok(docs) } else { Err(first_err) }
        }
    }
}

// ---------------------------------------------------------------------------
// Formatação

fn is_plain_key(k: &str) -> bool {
    let mut chars = k.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

pub fn format_key(k: &str) -> String {
    if is_plain_key(k) {
        k.to_string()
    } else {
        json_string(k)
    }
}

pub fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

pub fn format_double(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if v == 0.0 && v.is_sign_negative() {
        "-0.0".into()
    } else {
        let a = v.abs();
        if a != 0.0 && !(1e-6..1e16).contains(&a) {
            format!("{v:e}")
        } else if v.fract() == 0.0 {
            format!("{v:.1}")
        } else {
            format!("{v}")
        }
    }
}

pub fn format_date(d: &DateTime) -> String {
    match chrono::DateTime::from_timestamp_millis(d.timestamp_millis()) {
        Some(dt) => dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        None => d.timestamp_millis().to_string(),
    }
}

fn format_regex(r: &Regex) -> String {
    let mut out = String::from("/");
    for c in r.pattern.chars() {
        match c {
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    if r.pattern.is_empty() {
        out.push_str("(?:)");
    }
    out.push('/');
    out.push_str(&r.options);
    out
}

fn is_uuid_binary(b: &Binary) -> bool {
    b.subtype == BinarySubtype::Uuid && b.bytes.len() == 16
}

/// Formata valores escalares (não-documento, não-array).
fn scalar(b: &Bson) -> String {
    match b {
        Bson::Double(v) => format_double(*v),
        Bson::String(s) => json_string(s),
        Bson::Boolean(v) => v.to_string(),
        Bson::Null => "null".into(),
        Bson::Undefined => "undefined".into(),
        Bson::Int32(v) => v.to_string(),
        Bson::Int64(v) => format!("NumberLong({v})"),
        Bson::ObjectId(o) => format!("ObjectId(\"{}\")", o.to_hex()),
        Bson::DateTime(d) => match chrono::DateTime::from_timestamp_millis(d.timestamp_millis()) {
            Some(_) => format!("ISODate(\"{}\")", format_date(d)),
            None => format!("new Date({})", d.timestamp_millis()),
        },
        Bson::RegularExpression(r) => format_regex(r),
        Bson::Timestamp(t) => format!("Timestamp({{ t: {}, i: {} }})", t.time, t.increment),
        Bson::Decimal128(d) => format!("NumberDecimal(\"{d}\")"),
        Bson::Binary(bin) if is_uuid_binary(bin) => match bin.to_uuid() {
            Ok(u) => format!("UUID(\"{u}\")"),
            Err(_) => format!(
                "BinData({}, \"{}\")",
                u8::from(bin.subtype),
                base64::engine::general_purpose::STANDARD.encode(&bin.bytes)
            ),
        },
        Bson::Binary(bin) => format!(
            "BinData({}, \"{}\")",
            u8::from(bin.subtype),
            base64::engine::general_purpose::STANDARD.encode(&bin.bytes)
        ),
        Bson::JavaScriptCode(c) => format!("Code({})", json_string(c)),
        Bson::JavaScriptCodeWithScope(c) => {
            format!("Code({}, {})", json_string(&c.code), doc_to_shell_compact(&c.scope))
        }
        Bson::Symbol(s) => format!("Symbol({})", json_string(s)),
        Bson::MinKey => "MinKey()".into(),
        Bson::MaxKey => "MaxKey()".into(),
        Bson::DbPointer(p) => format!("{p:?}"),
        Bson::Document(d) => doc_to_shell_compact(d),
        Bson::Array(a) => {
            let parts: Vec<String> = a.iter().map(to_shell_compact).collect();
            format!("[{}]", parts.join(", "))
        }
    }
}

/// Representação de uma linha, ex.: `{ a: 1, b: "x" }`.
pub fn to_shell_compact(b: &Bson) -> String {
    scalar(b)
}

pub fn doc_to_shell_compact(d: &Document) -> String {
    if d.is_empty() {
        return "{}".into();
    }
    let parts: Vec<String> = d
        .iter()
        .map(|(k, v)| format!("{}: {}", format_key(k), to_shell_compact(v)))
        .collect();
    format!("{{ {} }}", parts.join(", "))
}

fn is_simple(b: &Bson) -> bool {
    !matches!(b, Bson::Document(d) if !d.is_empty()) && !matches!(b, Bson::Array(a) if !a.is_empty())
}

fn pretty_into(out: &mut String, b: &Bson, indent: usize) {
    match b {
        Bson::Document(d) if !d.is_empty() => {
            out.push_str("{\n");
            let n = d.len();
            for (i, (k, v)) in d.iter().enumerate() {
                push_indent(out, indent + 1);
                out.push_str(&format_key(k));
                out.push_str(": ");
                pretty_into(out, v, indent + 1);
                if i + 1 < n {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, indent);
            out.push('}');
        }
        Bson::Array(a) if !a.is_empty() => {
            let inline = a.iter().all(is_simple) && {
                let s = to_shell_compact(b);
                s.len() <= 80
            };
            if inline {
                out.push_str(&to_shell_compact(b));
                return;
            }
            out.push_str("[\n");
            let n = a.len();
            for (i, v) in a.iter().enumerate() {
                push_indent(out, indent + 1);
                pretty_into(out, v, indent + 1);
                if i + 1 < n {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, indent);
            out.push(']');
        }
        other => out.push_str(&scalar(other)),
    }
}

fn push_indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push_str("  ");
    }
}

/// Representação multilinha (indentada) de um valor.
pub fn to_shell_pretty(b: &Bson) -> String {
    let mut out = String::new();
    pretty_into(&mut out, b, 0);
    out
}

pub fn doc_to_shell_pretty(d: &Document) -> String {
    let mut out = String::new();
    pretty_into(&mut out, &Bson::Document(d.clone()), 0);
    out
}

pub fn docs_to_shell_pretty(docs: &[Document]) -> String {
    let mut out = String::from("[\n");
    for (i, d) in docs.iter().enumerate() {
        let s = doc_to_shell_pretty(d);
        for line in s.lines() {
            let _ = writeln!(out, "  {line}");
        }
        if i + 1 < docs.len() {
            out.pop();
            out.push_str(",\n");
        }
    }
    out.push(']');
    out
}

/// Nome legível do tipo BSON.
pub fn type_name(b: &Bson) -> &'static str {
    match b {
        Bson::Double(_) => "Double",
        Bson::String(_) => "String",
        Bson::Array(_) => "Array",
        Bson::Document(_) => "Object",
        Bson::Boolean(_) => "Boolean",
        Bson::Null => "Null",
        Bson::RegularExpression(_) => "Regex",
        Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_) => "Code",
        Bson::Int32(_) => "Int32",
        Bson::Int64(_) => "Int64",
        Bson::Timestamp(_) => "Timestamp",
        Bson::Binary(b) if is_uuid_binary(b) => "UUID",
        Bson::Binary(_) => "Binary",
        Bson::ObjectId(_) => "ObjectId",
        Bson::DateTime(_) => "Date",
        Bson::Symbol(_) => "Symbol",
        Bson::Decimal128(_) => "Decimal128",
        Bson::Undefined => "Undefined",
        Bson::MaxKey => "MaxKey",
        Bson::MinKey => "MinKey",
        Bson::DbPointer(_) => "DBPointer",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bson::doc;

    #[test]
    fn relaxed_syntax() {
        let d = parse_document("{ name: 'ana', age: {$gt: 5,}, 'a.b': true, tags: [1, 2.5, -3] }").unwrap();
        assert_eq!(
            d,
            doc! { "name": "ana", "age": { "$gt": 5 }, "a.b": true, "tags": [1, 2.5, -3] }
        );
        assert_eq!(parse_document("   ").unwrap(), Document::new());
        let d = parse_document("{a.b.c: 1}").unwrap();
        assert!(d.contains_key("a.b.c"));
    }

    #[test]
    fn constructors() {
        let d = parse_document(
            r#"{ _id: ObjectId("64b7f0c2a1b2c3d4e5f60718"), d: ISODate("2024-01-02T03:04:05.006Z"),
                 l: NumberLong("9007199254740993"), i: NumberInt(7), dec: NumberDecimal("1.50"),
                 u: UUID("123e4567-e89b-12d3-a456-426614174000"), r: /ab\/c[/]/i, t: Timestamp({t: 10, i: 2}),
                 n: new Date(0), dd: ISODate("2024-05-06") }"#,
        )
        .unwrap();
        assert!(matches!(d.get("_id"), Some(Bson::ObjectId(_))));
        assert_eq!(format_date(d.get_datetime("d").unwrap()), "2024-01-02T03:04:05.006Z");
        assert_eq!(d.get_i64("l").unwrap(), 9007199254740993);
        assert_eq!(d.get_i32("i").unwrap(), 7);
        match d.get("r").unwrap() {
            Bson::RegularExpression(r) => {
                assert_eq!(r.pattern, "ab/c[/]");
                assert_eq!(r.options, "i");
            }
            _ => panic!(),
        }
        assert_eq!(format_date(d.get_datetime("dd").unwrap()), "2024-05-06T00:00:00.000Z");
    }

    #[test]
    fn extjson() {
        let d = parse_document(
            r#"{"_id": {"$oid": "64b7f0c2a1b2c3d4e5f60718"}, "n": {"$numberLong": "12"},
                "d": {"$date": {"$numberLong": "1000"}}, "x": {"$gt": 1}}"#,
        )
        .unwrap();
        assert!(matches!(d.get("_id"), Some(Bson::ObjectId(_))));
        assert_eq!(d.get_i64("n").unwrap(), 12);
        assert_eq!(d.get_datetime("d").unwrap().timestamp_millis(), 1000);
        assert_eq!(d.get_document("x").unwrap(), &doc! {"$gt": 1});
    }

    #[test]
    fn roundtrip() {
        let original = doc! {
            "_id": ObjectId::new(),
            "s": "olá \"mundo\"\n",
            "i": 5, "l": 5_i64, "f": 5.0, "g": 0.1, "big": 1e300,
            "d": DateTime::from_millis(1_700_000_000_123),
            "nested": { "a": [1, { "b": null }], "c-d": true },
            "empty": {}, "arr": [],
            "re": Regex { pattern: "a/b".into(), options: "im".into() },
            "dec": "3.14".parse::<Decimal128>().unwrap(),
            "bin": Binary { subtype: BinarySubtype::Generic, bytes: vec![1, 2, 3] },
            "uuid": Binary::from_uuid(bson::Uuid::new()),
            "ts": Timestamp { time: 1, increment: 2 },
            "min": Bson::MinKey,
        };
        let pretty = doc_to_shell_pretty(&original);
        assert_eq!(parse_document(&pretty).unwrap(), original, "{pretty}");
        let compact = doc_to_shell_compact(&original);
        assert_eq!(parse_document(&compact).unwrap(), original, "{compact}");
    }

    #[test]
    fn errors_have_position() {
        let e = parse_document("{\n  a: 1,\n  b: }").unwrap_err();
        assert_eq!(e.line, 3);
        assert!(parse_document("{a: foo}").is_err());
        assert!(parse_document("{a: 1} x").is_err());
        assert!(parse_document("{a: 1, a: 2}").is_err());
    }

    #[test]
    fn multiple_documents() {
        assert_eq!(parse_documents("{a: 1}\n{a: 2}\n").unwrap().len(), 2);
        assert_eq!(parse_documents("[{a: 1}, {a: 2}]").unwrap().len(), 2);
        assert_eq!(parse_pipeline("[{$match: {}}, {$limit: 1}]").unwrap().len(), 2);
        assert_eq!(parse_pipeline("{$match: {}} // comentário").unwrap().len(), 1);
    }
}
