//! A tolerant, read-only outline of a ServiceNow script include.
//!
//! A small JavaScript tokenizer (strings, template literals, comments, and
//! regex literals are skipped as units) finds the object literal assigned to
//! `X.prototype`, either directly or as the second argument of
//! `Object.extendsObject(Base, {...})`, and lists its members with their line
//! spans. The spans let `include show --source-of` return exactly one member's
//! source. Nothing is evaluated; unusual code yields an empty or partial
//! outline rather than an error.

/// Nested keys listed for an object-valued member before eliding the rest.
const MAX_OBJECT_KEYS: usize = 12;

/// Literal values longer than this are not echoed in the outline.
const MAX_LITERAL_CHARS: usize = 60;

/// Script include classes referenced by one source are capped so a huge
/// script cannot flood the output.
pub const MAX_REFERENCES: usize = 40;

/// What a prototype member holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberKind {
    /// `name: function(params) {...}` or `name(params) {...}`.
    Function { params: String },
    /// `name: {...}`, with its own keys (at most [`MAX_OBJECT_KEYS`]).
    Object { keys: Vec<String>, more: usize },
    /// Anything else; short literals (strings, numbers, booleans, null) are kept.
    Value { literal: Option<String> },
}

/// One member of a script include's prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub kind: MemberKind,
    /// 1-based first line (the key).
    pub start_line: usize,
    /// 1-based last line of the value.
    pub end_line: usize,
}

impl Member {
    /// Compact one-line form, e.g. `43-59 initialize(changeRequestGr)`,
    /// `27-69 assess{nextState,draft,authorize}`, `38 NAME="value"`.
    pub fn summary(&self) -> String {
        let lines = if self.end_line > self.start_line {
            format!("{}-{}", self.start_line, self.end_line)
        } else {
            self.start_line.to_string()
        };
        match &self.kind {
            MemberKind::Function { params } => format!("{lines} {}({params})", self.name),
            MemberKind::Object { keys, more } => {
                let more = if *more > 0 {
                    format!(",+{more}")
                } else {
                    String::new()
                };
                format!("{lines} {}{{{}{more}}}", self.name, keys.join(","))
            }
            MemberKind::Value {
                literal: Some(literal),
            } => format!("{lines} {}={literal}", self.name),
            MemberKind::Value { literal: None } => format!("{lines} {}", self.name),
        }
    }
}

/// Members of the script's prototype object literal in source order. The
/// conventional `type: "<ClassName>"` member is omitted.
pub fn outline(script: &str) -> Vec<Member> {
    let tokens = lex(script);
    let Some(open) = find_prototype_object(script, &tokens) else {
        return Vec::new();
    };
    let lines = LineIndex::new(script);
    object_members(script, &tokens, open)
        .into_iter()
        .filter_map(|entry| member(script, &tokens, &lines, &entry))
        .filter(|member| {
            !(member.name == "type"
                && matches!(&member.kind, MemberKind::Value { literal: Some(text) } if text.starts_with(['"', '\''])))
        })
        .collect()
}

/// Candidate classes the script mentions as `new X(` or `X.member`, in order
/// of first appearance: identifiers starting with an uppercase letter, minus
/// JavaScript built-ins and `Glide*` platform APIs. They are names only; none
/// is verified to be a script include.
pub fn references(script: &str) -> Vec<String> {
    let tokens = lex(script);
    let text = |index: usize| tokens.get(index).map(|token| token.text(script));
    let mut found: Vec<String> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Ident {
            continue;
        }
        let previous = index.checked_sub(1).and_then(text);
        if previous == Some(".") {
            continue;
        }
        let name = token.text(script);
        let is_new = previous == Some("new");
        let is_member = text(index + 1) == Some(".");
        if !(is_new || is_member) {
            continue;
        }
        // `new global.X(` names X in scope `global`.
        let name = if is_new && is_member && text(index + 3) == Some("(") {
            match tokens.get(index + 2) {
                Some(next) if next.kind == Kind::Ident => {
                    format!("{name}.{}", next.text(script))
                }
                _ => name.to_string(),
            }
        } else {
            name.to_string()
        };
        let simple = name.rsplit('.').next().unwrap_or(&name);
        if !simple.starts_with(|ch: char| ch.is_ascii_uppercase())
            || is_builtin(simple)
            || found.contains(&name)
        {
            continue;
        }
        found.push(name);
        if found.len() >= MAX_REFERENCES {
            break;
        }
    }
    found
}

fn is_builtin(name: &str) -> bool {
    const BUILTINS: &[&str] = &[
        "Array",
        "Boolean",
        "Class",
        "Date",
        "Error",
        "Function",
        "Infinity",
        "JSON",
        "Map",
        "Math",
        "NaN",
        "Number",
        "Object",
        "Packages",
        "Promise",
        "RangeError",
        "RegExp",
        "SNC",
        "Set",
        "String",
        "Symbol",
        "TypeError",
        "XMLDocument",
        "XMLDocument2",
    ];
    name.starts_with("Glide") || BUILTINS.contains(&name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ident,
    Number,
    String,
    Template,
    Regex,
    Punct,
}

#[derive(Debug, Clone, Copy)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

impl Token {
    fn text<'a>(&self, source: &'a str) -> &'a str {
        source.get(self.start..self.end).unwrap_or_default()
    }

    fn is_punct(&self, source: &str, punct: &str) -> bool {
        self.kind == Kind::Punct && self.text(source) == punct
    }
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

/// Keywords after which `/` starts a regular expression, not a division.
fn regex_may_follow(previous: Option<(&Token, &str)>) -> bool {
    match previous {
        None => true,
        Some((token, text)) => match token.kind {
            Kind::Punct => !matches!(text, ")" | "]" | "}"),
            Kind::Ident => matches!(
                text,
                "return"
                    | "typeof"
                    | "case"
                    | "do"
                    | "else"
                    | "in"
                    | "of"
                    | "new"
                    | "delete"
                    | "void"
                    | "throw"
                    | "instanceof"
                    | "yield"
                    | "await"
            ),
            _ => false,
        },
    }
}

/// Tokenize JavaScript leniently: unterminated constructs end at the end of
/// the line (strings, regexes) or the input (comments, templates).
fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens: Vec<Token> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let start = index;
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index = source[index + 2..]
                .find("*/")
                .map_or(bytes.len(), |end| index + 2 + end + 2);
            continue;
        }
        let kind = if byte == b'"' || byte == b'\'' {
            index = skip_quoted(bytes, index, byte, true);
            Kind::String
        } else if byte == b'`' {
            index = skip_quoted(bytes, index, b'`', false);
            Kind::Template
        } else if byte == b'/'
            && regex_may_follow(tokens.last().map(|token| (token, token.text(source))))
        {
            match skip_regex(bytes, index) {
                Some(end) => {
                    index = end;
                    Kind::Regex
                }
                None => {
                    index += 1;
                    Kind::Punct
                }
            }
        } else if byte.is_ascii_digit() {
            while index < bytes.len() && (is_ident_byte(bytes[index]) || bytes[index] == b'.') {
                index += 1;
            }
            Kind::Number
        } else if is_ident_byte(byte) {
            while index < bytes.len() && is_ident_byte(bytes[index]) {
                index += 1;
            }
            Kind::Ident
        } else {
            index += 1;
            Kind::Punct
        };
        tokens.push(Token {
            kind,
            start,
            end: index,
        });
    }
    tokens
}

/// End (exclusive) of a quoted literal starting at `start`.
fn skip_quoted(bytes: &[u8], start: usize, quote: u8, stop_at_newline: bool) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'\n' if stop_at_newline => return index,
            byte if byte == quote => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

/// End (exclusive) of a regex literal starting at `start`, with its flags,
/// or `None` when the line ends first (then `/` was a division).
fn skip_regex(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start + 1;
    let mut in_class = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'\n' => return None,
            b'[' => {
                in_class = true;
                index += 1;
            }
            b']' => {
                in_class = false;
                index += 1;
            }
            b'/' if !in_class => {
                index += 1;
                while index < bytes.len() && is_ident_byte(bytes[index]) {
                    index += 1;
                }
                return Some(index);
            }
            _ => index += 1,
        }
    }
    None
}

/// Index of the `{` token that opens the prototype object literal.
fn find_prototype_object(source: &str, tokens: &[Token]) -> Option<usize> {
    let text = |index: usize| tokens.get(index).map_or("", |token| token.text(source));
    for index in 1..tokens.len() {
        if text(index) != "prototype"
            || text(index - 1) != "."
            || text(index + 1) != "="
            || text(index + 2) == "="
        {
            continue;
        }
        let value = index + 2;
        if text(value) == "{" {
            return Some(value);
        }
        // Object.extendsObject(Base, {...})
        if text(value) == "Object"
            && text(value + 1) == "."
            && text(value + 2) == "extendsObject"
            && text(value + 3) == "("
        {
            let mut depth = 0usize;
            for (offset, token) in tokens.iter().enumerate().skip(value + 4) {
                match token.text(source) {
                    "(" | "[" | "{" if token.kind == Kind::Punct => depth += 1,
                    ")" | "]" | "}" if token.kind == Kind::Punct => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    "," if token.kind == Kind::Punct && depth == 0 => {
                        return (text(offset + 1) == "{").then_some(offset + 1);
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

/// One `key: value` (or shorthand method) entry of an object literal, as
/// token indexes: the key and the inclusive value range.
struct Entry {
    key: usize,
    value_start: usize,
    value_end: usize,
    shorthand: bool,
}

/// Entries of the object literal opened at `open` (depth 1 only).
fn object_members(source: &str, tokens: &[Token], open: usize) -> Vec<Entry> {
    let text = |index: usize| tokens.get(index).map_or("", |token| token.text(source));
    let mut entries = Vec::new();
    let mut index = open + 1;
    while index < tokens.len() {
        if tokens[index].is_punct(source, "}") {
            break;
        }
        if tokens[index].is_punct(source, ",") {
            index += 1;
            continue;
        }
        // `get x() {}`, `async x() {}` and similar: the name is the second ident.
        let mut key = index;
        if matches!(text(key), "get" | "set" | "async" | "static")
            && tokens
                .get(key + 1)
                .is_some_and(|token| token.kind == Kind::Ident)
            && text(key + 2) == "("
        {
            key += 1;
        }
        let key_token = tokens[key];
        let is_key = matches!(key_token.kind, Kind::Ident | Kind::String | Kind::Number);
        let (value_start, shorthand) = if is_key && text(key + 1) == ":" {
            (key + 2, false)
        } else if key_token.kind == Kind::Ident && text(key + 1) == "(" {
            (key + 1, true)
        } else {
            // Not a member we understand (spread, computed key): skip it.
            (key, false)
        };
        let value_end = value_end(source, tokens, value_start);
        if is_key && (shorthand || value_start == key + 2) && value_start < tokens.len() {
            entries.push(Entry {
                key,
                value_start,
                value_end,
                shorthand,
            });
        }
        index = value_end + 1;
    }
    entries
}

/// Last token index of the value starting at `start`: just before the next
/// `,` or closing bracket at nesting depth zero.
fn value_end(source: &str, tokens: &[Token], start: usize) -> usize {
    let mut depth = 0usize;
    let mut index = start;
    while index < tokens.len() {
        let token = tokens[index];
        if token.kind == Kind::Punct {
            match token.text(source) {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => {
                    if depth == 0 {
                        return index.saturating_sub(1).max(start);
                    }
                    depth -= 1;
                }
                "," if depth == 0 => return index.saturating_sub(1).max(start),
                _ => {}
            }
        }
        index += 1;
    }
    tokens.len().saturating_sub(1).max(start)
}

fn member(source: &str, tokens: &[Token], lines: &LineIndex, entry: &Entry) -> Option<Member> {
    let text = |index: usize| tokens.get(index).map_or("", |token| token.text(source));
    let key = tokens.get(entry.key)?;
    let name = match key.kind {
        Kind::String => key
            .text(source)
            .trim_matches(|ch| ch == '"' || ch == '\'')
            .to_string(),
        _ => key.text(source).to_string(),
    };
    let params_from = if entry.shorthand {
        Some(entry.value_start)
    } else if text(entry.value_start) == "function" {
        // `function (` or `function name(`
        (entry.value_start + 1..=entry.value_start + 2).find(|&index| text(index) == "(")
    } else if text(entry.value_start) == "async" && text(entry.value_start + 1) == "function" {
        (entry.value_start + 2..=entry.value_start + 3).find(|&index| text(index) == "(")
    } else {
        None
    };
    let kind = if let Some(open) = params_from {
        let close = (open..=entry.value_end)
            .find(|&index| text(index) == ")")
            .unwrap_or(open);
        let params = tokens
            .get(open + 1..close)
            .unwrap_or_default()
            .iter()
            .map(|token| token.text(source))
            .collect::<Vec<_>>()
            .join("")
            .replace(',', ", ");
        MemberKind::Function { params }
    } else if text(entry.value_start) == "{" && tokens[entry.value_start].kind == Kind::Punct {
        let nested = object_members(source, tokens, entry.value_start);
        let mut keys: Vec<String> = nested
            .iter()
            .filter_map(|nested| {
                let key = tokens.get(nested.key)?.text(source);
                let key = key.trim_matches(|ch| ch == '"' || ch == '\'');
                Some(match short_list(source, tokens, nested) {
                    Some(list) => format!("{key}=[{list}]"),
                    None => key.to_string(),
                })
            })
            .collect();
        let more = keys.len().saturating_sub(MAX_OBJECT_KEYS);
        keys.truncate(MAX_OBJECT_KEYS);
        MemberKind::Object { keys, more }
    } else {
        let value = tokens[entry.value_start];
        let single = entry.value_start == entry.value_end
            || (text(entry.value_start) == "-" && entry.value_start + 1 == entry.value_end);
        let literal = single
            .then(|| {
                source
                    .get(value.start..tokens[entry.value_end].end)
                    .unwrap_or_default()
            })
            .filter(|literal| {
                literal.chars().count() <= MAX_LITERAL_CHARS
                    && (matches!(
                        value.kind,
                        Kind::String | Kind::Number | Kind::Punct | Kind::Template
                    ) || matches!(*literal, "true" | "false" | "null" | "undefined"))
                    && !literal.contains('\n')
            })
            .map(str::to_string);
        MemberKind::Value { literal }
    };
    let end = tokens
        .get(entry.value_end)
        .map_or(key.end, |token| token.end);
    Some(Member {
        name,
        kind,
        start_line: lines.line_of(key.start),
        end_line: lines.line_of(end.saturating_sub(1).max(key.start)),
    })
}

/// `a,b` for a short array of string or number literals such as
/// `nextState: [ "authorize" ]`, so state-machine style objects show their
/// targets in the outline.
fn short_list(source: &str, tokens: &[Token], entry: &Entry) -> Option<String> {
    let value = tokens.get(entry.value_start..=entry.value_end)?;
    let (first, rest) = value.split_first()?;
    let (last, items) = rest.split_last()?;
    if !first.is_punct(source, "[") || !last.is_punct(source, "]") {
        return None;
    }
    let mut list = Vec::new();
    for (index, token) in items.iter().enumerate() {
        let separator = index % 2 == 1;
        match token.kind {
            Kind::Punct if separator && token.text(source) == "," => {}
            Kind::String | Kind::Number if !separator => list.push(
                token
                    .text(source)
                    .trim_matches(|ch| ch == '"' || ch == '\''),
            ),
            _ => return None,
        }
    }
    let list = list.join(",");
    (list.chars().count() <= MAX_LITERAL_CHARS).then_some(list)
}

/// Byte offset to 1-based line number.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(source: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index + 1),
        );
        Self { starts }
    }

    fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDLER_SNC: &str = "var H = Class.create();\n\
H.DRAFT = \"draft\";\n\
H.prototype = {\n\
\t/** A comment with { braces } and 'quotes' */\n\
\tSTATE_NAMES: {\n\
\t\t\"-5\": H.DRAFT,\n\
\t\t\"-4\": H.ASSESS\n\
\t},\n\
\tDEFAULT_MODEL_CLASS: \"ChangeRequestStateModel_normal\",\n\
\tSTATE_VALUES: null,\n\
\tinitialize: function(gr, opts) {\n\
\t\tthis._re = /[}{]\\//g;\n\
\t\tthis.s = \"} not a close\";\n\
\t\tthis._utils = new ChangeUtils();\n\
\t},\n\
\t_resetModel: function() {\n\
\t\tswitch (this._gr.type + \"\") {\n\
\t\t\tcase \"normal\": this._model = new ChangeRequestStateModel_normal(this._gr); break;\n\
\t\t}\n\
\t\tvar x = GSLog.DEBUG / 2;\n\
\t\tvar g = new GlideRecord(\"x\");\n\
\t},\n\
\tshort(a) { return a; },\n\
\ttype: \"H\"\n\
};";

    #[test]
    fn outline_lists_members_with_line_spans_and_kinds() {
        let members = outline(HANDLER_SNC);
        let summaries: Vec<String> = members.iter().map(Member::summary).collect();
        assert_eq!(
            summaries,
            [
                "5-8 STATE_NAMES{-5,-4}",
                "9 DEFAULT_MODEL_CLASS=\"ChangeRequestStateModel_normal\"",
                "10 STATE_VALUES=null",
                "11-15 initialize(gr, opts)",
                "16-22 _resetModel()",
                "23 short(a)",
            ]
        );
    }

    #[test]
    fn outline_reads_extends_object_prototype_and_nested_state_objects() {
        let script = "var M = Class.create();\nM.prototype = Object.extendsObject(global.Base, {\n    assess: {\n        nextState: [ \"authorize\" ],\n        authorize: {\n            canMove: function() { return true; }\n        },\n        canceled: {}\n    },\n    closed: {},\n    type: \"M\"\n});";
        let summaries: Vec<String> = outline(script).iter().map(Member::summary).collect();
        assert_eq!(
            summaries,
            [
                "3-9 assess{nextState=[authorize],authorize,canceled}",
                "10 closed{}"
            ]
        );
    }

    #[test]
    fn outline_without_prototype_is_empty() {
        assert!(outline("gs.info('hello');").is_empty());
        assert!(outline("").is_empty());
        assert!(outline("X.prototype = Object.extendsObject(").is_empty());
    }

    #[test]
    fn outline_elides_long_key_lists() {
        let keys: Vec<String> = (0..15).map(|i| format!("k{i}: 1")).collect();
        let script = format!("X.prototype = {{ big: {{ {} }} }};", keys.join(", "));
        let members = outline(&script);
        assert_eq!(
            members[0].summary(),
            "1 big{k0,k1,k2,k3,k4,k5,k6,k7,k8,k9,k10,k11,+3}"
        );
    }

    #[test]
    fn references_lists_candidate_classes_in_order() {
        assert_eq!(
            references(HANDLER_SNC),
            [
                "H",
                "ChangeUtils",
                "ChangeRequestStateModel_normal",
                "GSLog"
            ]
        );
        assert_eq!(
            references("var a = new global.MyUtil(); var b = new x_app.Other(1); Math.max(1);"),
            ["global.MyUtil", "x_app.Other"]
        );
    }

    #[test]
    fn lexer_survives_unterminated_constructs() {
        for script in [
            "X.prototype = { a: \"open",
            "/* never closed",
            "`tpl",
            "a = /re",
        ] {
            let _ = outline(script);
            let _ = references(script);
        }
    }
}
