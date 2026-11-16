//! Doc comment sections: the structure docstring conventions put in a doc
//! comment (parameters, return value, exceptions, examples), split out of
//! its text. The pieces stay in the comment's markup (Markdown,
//! reStructuredText, ...); rendering them is the caller's job.
//!
//! ```
//! use lsdocs_apidoc::docstring::{parse, DocStyle};
//!
//! let docs = parse("Add two numbers.\n\nArgs:\n    a (int): The first.\n    b: The second.\n\nReturns:\n    int: The sum.\n", DocStyle::Auto);
//! assert_eq!(docs.body, "Add two numbers.");
//! assert_eq!(docs.params[0].name.as_deref(), Some("a"));
//! assert_eq!(docs.params[0].ty.as_deref(), Some("int"));
//! assert_eq!(docs.returns.unwrap().text, "The sum.");
//! ```

use serde::{Deserialize, Serialize};

/// The docstring convention a comment follows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocStyle {
    /// Find out from the text (the default).
    #[default]
    Auto,
    /// No sections: all of it is the body.
    Plain,
    /// Google style: `Args:`, `Returns:`, ... with indented entries.
    Google,
    /// NumPy style: `Parameters` underlined with `----`.
    Numpy,
    /// Sphinx field lists: `:param x:`, `:returns:`, `:raises E:`.
    Sphinx,
    /// Tags: Javadoc, JSDoc / TSDoc and Doxygen (`@param x`, `\param x`).
    Tags,
}

/// One documented parameter, exception, return value and the like.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The name (a parameter's), if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The type, if the comment gives one (an exception's type is here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// The description, in the comment's markup.
    #[serde(default)]
    pub text: String,
}

/// A doc comment split into its parts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Docs {
    /// The text outside every section: the summary and description.
    pub body: String,
    /// Parameters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Entry>,
    /// Generic or template parameters (`@tparam`, `@typeParam`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub type_params: Vec<Entry>,
    /// The return value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<Entry>,
    /// What a generator yields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yields: Option<Entry>,
    /// Exceptions or errors raised, by type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raises: Vec<Entry>,
    /// Attributes (of a class) documented in its comment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<Entry>,
    /// Examples, each in the comment's markup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
    /// Names or links of related items.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub see_also: Vec<String>,
    /// Why it is deprecated (empty if the comment says only that it is).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    /// The version it appeared in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Other sections (`Notes`, `Warning`, ...), with their titles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<(String, String)>,
}

/// Splits a doc comment (comment markers already removed) into its parts.
pub fn parse(text: &str, style: DocStyle) -> Docs {
    let text = dedent(text);
    match resolve_style(&text, style) {
        DocStyle::Google => google(&text),
        DocStyle::Numpy => numpy(&text),
        DocStyle::Sphinx => sphinx(&text),
        DocStyle::Tags => tags(&text),
        DocStyle::Plain | DocStyle::Auto => Docs {
            body: text.trim().to_owned(),
            ..Docs::default()
        },
    }
}

/// The style `text` is in, for [`DocStyle::Auto`].
pub fn detect(text: &str) -> DocStyle {
    resolve_style(&dedent(text), DocStyle::Auto)
}

fn resolve_style(text: &str, style: DocStyle) -> DocStyle {
    if style != DocStyle::Auto {
        return style;
    }
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if numpy_section(trimmed).is_some()
            && lines
                .get(i + 1)
                .is_some_and(|next| is_underline(next.trim()))
        {
            return DocStyle::Numpy;
        }
    }
    for line in &lines {
        let trimmed = line.trim();
        if !line.starts_with(char::is_whitespace)
            && trimmed
                .strip_suffix(':')
                .is_some_and(|title| google_section(title).is_some())
        {
            return DocStyle::Google;
        }
    }
    if lines.iter().any(|l| {
        let t = l.trim_start();
        [":param", ":return", ":raises", ":type", ":rtype"]
            .iter()
            .any(|p| t.starts_with(p))
    }) {
        return DocStyle::Sphinx;
    }
    if lines.iter().any(|l| tag(l.trim_start()).is_some()) {
        return DocStyle::Tags;
    }
    DocStyle::Plain
}

/// `text` without the indentation all its lines (but the first) share:
/// Python docstrings indent every line after the first.
fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let indent = lines
        .iter()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            out.push(line.trim_start().to_owned());
        } else {
            out.push(line.get(indent..).unwrap_or(line.trim_start()).to_owned());
        }
    }
    out.join("\n")
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Joins the lines of a block, dedented, trimmed.
fn block(lines: &[&str]) -> String {
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| l.get(indent..).unwrap_or(l.trim_start()))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// What a section title (Google or NumPy) is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Params,
    TypeParams,
    Returns,
    Yields,
    Raises,
    Attributes,
    Examples,
    SeeAlso,
    Other,
}

fn google_section(title: &str) -> Option<Section> {
    Some(match title.trim().to_lowercase().as_str() {
        "args" | "arguments" | "parameters" | "params" | "keyword args" | "keyword arguments"
        | "other parameters" => Section::Params,
        "type parameters" => Section::TypeParams,
        "returns" | "return" => Section::Returns,
        "yields" | "yield" => Section::Yields,
        "raises" | "raise" | "exceptions" | "throws" => Section::Raises,
        "attributes" => Section::Attributes,
        "example" | "examples" => Section::Examples,
        "see also" => Section::SeeAlso,
        "note" | "notes" | "warning" | "warnings" | "todo" | "references" | "methods" | "warns"
        | "caution" | "tip" | "hint" | "important" | "attention" | "danger" | "error" => {
            Section::Other
        }
        _ => return None,
    })
}

fn numpy_section(title: &str) -> Option<Section> {
    google_section(title)
}

fn is_underline(line: &str) -> bool {
    line.len() >= 3 && line.chars().all(|c| c == '-')
}

/// Adds a section's text to `docs`.
fn add_section(docs: &mut Docs, kind: Section, title: &str, entries: Vec<Entry>, text: String) {
    match kind {
        Section::Params => docs.params.extend(entries),
        Section::TypeParams => docs.type_params.extend(entries),
        Section::Attributes => docs.attributes.extend(entries),
        Section::Raises => docs.raises.extend(entries),
        Section::Returns => docs.returns = merge_return(entries, text),
        Section::Yields => docs.yields = merge_return(entries, text),
        Section::Examples => docs.examples.push(text),
        Section::SeeAlso => docs.see_also.extend(
            text.lines()
                .flat_map(|l| l.split(','))
                .map(|s| s.split(':').next().unwrap_or_default().trim().to_owned())
                .filter(|s| !s.is_empty()),
        ),
        Section::Other => docs.sections.push((title.trim().to_owned(), text)),
    }
}

/// A return value from a section's entries (for typed ones) or its text.
fn merge_return(entries: Vec<Entry>, text: String) -> Option<Entry> {
    match entries.into_iter().next() {
        Some(entry) => Some(Entry {
            name: None,
            ..entry
        }),
        None => Some(Entry {
            text,
            ..Entry::default()
        }),
    }
}

// -- Google ------------------------------------------------------------------

fn google(text: &str) -> Docs {
    let mut docs = Docs::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut body = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let header = (!line.starts_with(char::is_whitespace))
            .then(|| line.trim().strip_suffix(':'))
            .flatten()
            .and_then(|t| google_section(t).map(|k| (t, k)));
        let Some((title, kind)) = header else {
            body.push(line);
            i += 1;
            continue;
        };
        i += 1;
        let start = i;
        while i < lines.len()
            && (lines[i].trim().is_empty() || lines[i].starts_with(char::is_whitespace))
        {
            i += 1;
        }
        let content = &lines[start..i];
        let entries = match kind {
            Section::Params | Section::TypeParams | Section::Attributes => {
                google_entries(content, true)
            }
            Section::Raises => google_entries(content, false)
                .into_iter()
                .map(|e| Entry {
                    ty: e.name,
                    name: None,
                    text: e.text,
                })
                .collect(),
            Section::Returns | Section::Yields => google_return(content),
            _ => Vec::new(),
        };
        add_section(&mut docs, kind, title, entries, block(content));
    }
    docs.body = block(&body);
    docs
}

/// Entries of a Google section: `name (type): text` lines, with more
/// indented continuation lines.
fn google_entries(lines: &[&str], typed: bool) -> Vec<Entry> {
    let base = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .min()
        .unwrap_or(0);
    let mut out: Vec<(Entry, Vec<&str>)> = Vec::new();
    for line in lines {
        if !line.trim().is_empty() && indent_of(line) == base {
            let (head, rest) = line.trim().split_once(':').unwrap_or((line.trim(), ""));
            let (name, ty) = match head.split_once('(') {
                Some((name, ty)) if typed => (
                    name.trim().to_owned(),
                    Some(ty.trim_end_matches(')').trim().to_owned()),
                ),
                _ => (head.trim().to_owned(), None),
            };
            out.push((
                Entry {
                    name: Some(name),
                    ty,
                    text: String::new(),
                },
                vec![rest.trim()],
            ));
        } else if let Some((_, text)) = out.last_mut() {
            text.push(line);
        }
    }
    out.into_iter()
        .map(|(mut entry, text)| {
            entry.text = join_continued(&text);
            entry
        })
        .collect()
}

/// A Google `Returns:` section: `type: text`, or only text.
fn google_return(lines: &[&str]) -> Vec<Entry> {
    let text = block(lines);
    let first = text.lines().next().unwrap_or_default();
    match first.split_once(':') {
        Some((ty, rest)) if !ty.contains(' ') || ty.contains(['[', '<', '|']) => {
            let mut description = vec![rest.trim()];
            description.extend(text.lines().skip(1));
            vec![Entry {
                name: None,
                ty: Some(ty.trim().to_owned()),
                text: join_continued(&description),
            }]
        }
        _ => vec![Entry {
            text,
            ..Entry::default()
        }],
    }
}

/// A first line and continuation lines (any indentation) as one text,
/// keeping paragraph breaks.
fn join_continued(lines: &[&str]) -> String {
    let first = lines.first().copied().unwrap_or_default();
    let rest = block(&lines[1.min(lines.len())..]);
    match (first.trim().is_empty(), rest.is_empty()) {
        (_, true) => first.trim().to_owned(),
        (true, false) => rest,
        (false, false) if rest.starts_with('\n') => format!("{}\n{rest}", first.trim()),
        (false, false) => format!("{} {rest}", first.trim()),
    }
}

// -- NumPy -------------------------------------------------------------------

fn numpy(text: &str) -> Docs {
    let mut docs = Docs::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut body = Vec::new();
    let mut i = 0;
    let is_header = |i: usize| {
        numpy_section(lines[i].trim()).is_some()
            && lines.get(i + 1).is_some_and(|n| is_underline(n.trim()))
    };
    while i < lines.len() {
        if !is_header(i) {
            body.push(lines[i]);
            i += 1;
            continue;
        }
        let title = lines[i].trim();
        let kind = numpy_section(title).unwrap_or(Section::Other);
        i += 2;
        let start = i;
        while i < lines.len() && !is_header(i) {
            i += 1;
        }
        let content = &lines[start..i];
        let entries = match kind {
            Section::Params | Section::TypeParams | Section::Attributes => numpy_entries(content),
            Section::Returns | Section::Yields | Section::Raises => numpy_entries(content)
                .into_iter()
                .map(|e| match e.ty {
                    // `name : type`, or only a type.
                    Some(ty) => Entry {
                        name: e.name,
                        ty: Some(ty),
                        text: e.text,
                    },
                    None => Entry {
                        name: None,
                        ty: e.name,
                        text: e.text,
                    },
                })
                .collect(),
            _ => Vec::new(),
        };
        let entries = if kind == Section::Raises {
            entries
                .into_iter()
                .map(|e| Entry { name: None, ..e })
                .collect()
        } else {
            entries
        };
        add_section(&mut docs, kind, title, entries, block(content));
    }
    docs.body = block(&body);
    docs
}

/// Entries of a NumPy section: `name : type` lines (or only a name or a
/// type), each with an indented description below.
fn numpy_entries(lines: &[&str]) -> Vec<Entry> {
    let base = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .min()
        .unwrap_or(0);
    let mut out: Vec<(Entry, Vec<&str>)> = Vec::new();
    for line in lines {
        if !line.trim().is_empty() && indent_of(line) == base {
            let head = line.trim();
            let (name, ty) = match head.split_once(" : ").or_else(|| head.split_once(':')) {
                Some((name, ty)) => (name.trim().to_owned(), Some(ty.trim().to_owned())),
                None => (head.to_owned(), None),
            };
            out.push((
                Entry {
                    name: Some(name),
                    ty: ty.filter(|t| !t.is_empty()),
                    text: String::new(),
                },
                Vec::new(),
            ));
        } else if let Some((_, text)) = out.last_mut() {
            text.push(line);
        }
    }
    out.into_iter()
        .map(|(mut entry, text)| {
            entry.text = block(&text);
            entry
        })
        .collect()
}

// -- Sphinx field lists ----------------------------------------------------

fn sphinx(text: &str) -> Docs {
    let mut docs = Docs::default();
    let mut body = Vec::new();
    // (field name words, text lines)
    let mut fields: Vec<(Vec<String>, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if indent_of(line) == 0
            && let Some(rest) = trimmed.strip_prefix(':')
            && let Some((name, value)) = rest.split_once(':')
        {
            let words = name.split_whitespace().map(str::to_owned).collect();
            fields.push((words, vec![value.trim()]));
            continue;
        }
        match fields.last_mut() {
            Some((_, lines)) if line.trim().is_empty() || indent_of(line) > 0 => lines.push(line),
            _ => body.push(line),
        }
    }
    let mut types: Vec<(String, String)> = Vec::new();
    let mut rtype = None;
    for (words, lines) in fields {
        let text = join_continued(&lines);
        let kind = words.first().map_or("", String::as_str);
        match (kind, &words[1..]) {
            ("param" | "parameter" | "arg" | "argument" | "key" | "keyword", rest) => {
                let (ty, name) = match rest {
                    [name] => (None, name.clone()),
                    [ty @ .., name] if !ty.is_empty() => (Some(ty.join(" ")), name.clone()),
                    _ => continue,
                };
                docs.params.push(Entry {
                    name: Some(name),
                    ty,
                    text,
                });
            }
            ("type", [name]) => types.push((name.clone(), text)),
            ("returns" | "return", _) => {
                docs.returns = Some(Entry {
                    text,
                    ..Entry::default()
                })
            }
            ("rtype", _) => rtype = Some(text),
            ("raises" | "raise" | "except" | "exception", rest) => docs.raises.push(Entry {
                name: None,
                ty: (!rest.is_empty()).then(|| rest.join(" ")),
                text,
            }),
            ("yields" | "yield", _) => {
                docs.yields = Some(Entry {
                    text,
                    ..Entry::default()
                })
            }
            ("deprecated", _) => docs.deprecated = Some(text),
            ("versionadded" | "since", _) => docs.since = Some(text),
            ("seealso" | "see", _) => docs.see_also.push(text),
            (other, _) => docs.sections.push((other.to_owned(), text)),
        }
    }
    for (name, ty) in types {
        if let Some(p) = docs
            .params
            .iter_mut()
            .find(|p| p.name.as_deref() == Some(&name))
        {
            p.ty.get_or_insert(ty);
        }
    }
    if let Some(ty) = rtype {
        docs.returns.get_or_insert_with(Entry::default).ty = Some(ty);
    }
    docs.body = block(&body);
    docs
}

// -- Tags --------------------------------------------------------------------

/// The tag a line starts with (`@param`, `\return`), and the rest.
fn tag(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('@').or_else(|| line.strip_prefix('\\'))?;
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let name = &rest[..end];
    const KNOWN: &[&str] = &[
        "param",
        "arg",
        "argument",
        "tparam",
        "typeParam",
        "template",
        "return",
        "returns",
        "result",
        "throws",
        "throw",
        "exception",
        "raises",
        "deprecated",
        "since",
        "see",
        "sa",
        "example",
        "brief",
        "details",
        "note",
        "warning",
        "remark",
        "remarks",
        "yields",
        "property",
        "prop",
        "type",
    ];
    KNOWN.contains(&name).then(|| (name, &rest[end..]))
}

fn tags(text: &str) -> Docs {
    let mut docs = Docs::default();
    let mut body: Vec<&str> = Vec::new();
    let mut tagged: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut described: Vec<String> = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
        }
        match (!in_code).then(|| tag(line.trim_start())).flatten() {
            Some((name, rest)) => tagged.push((name, vec![rest])),
            None => match tagged.last_mut() {
                Some((_, lines)) => lines.push(line),
                None => body.push(line),
            },
        }
    }
    for (name, lines) in tagged {
        let raw = join_continued(&lines);
        match name {
            "param" | "arg" | "argument" | "tparam" | "typeParam" | "template" | "property"
            | "prop" => {
                let entry = tag_entry(&raw);
                match name {
                    "tparam" | "typeParam" | "template" => docs.type_params.push(entry),
                    "property" | "prop" => docs.attributes.push(entry),
                    _ => docs.params.push(entry),
                }
            }
            "return" | "returns" | "result" => {
                let (ty, text) = braced_type(&raw);
                docs.returns = Some(Entry {
                    name: None,
                    ty,
                    text,
                });
            }
            "yields" => {
                docs.yields = Some(Entry {
                    text: raw,
                    ..Entry::default()
                })
            }
            "throws" | "throw" | "exception" | "raises" => {
                let (ty, text) = match braced_type(&raw) {
                    (Some(ty), text) => (Some(ty), text),
                    (None, text) => match text.split_once(char::is_whitespace) {
                        Some((ty, rest)) => (Some(ty.to_owned()), rest.trim().to_owned()),
                        None if !text.is_empty() => (Some(text.clone()), String::new()),
                        None => (None, text),
                    },
                };
                docs.raises.push(Entry {
                    name: None,
                    ty,
                    text,
                });
            }
            "deprecated" => docs.deprecated = Some(raw),
            "since" => docs.since = Some(raw),
            "see" | "sa" => docs.see_also.push(raw),
            "example" => docs.examples.push(block(&lines)),
            "brief" | "details" => described.push(raw),
            "note" | "warning" | "remark" | "remarks" => {
                let mut title = name.to_owned();
                if let Some(first) = title.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                docs.sections.push((title, raw));
            }
            _ => docs.sections.push((name.to_owned(), raw)),
        }
    }
    // `@brief` and `@details` text is body text, after any untagged text.
    let mut parts = vec![block(&body)];
    parts.extend(described);
    parts.retain(|p| !p.is_empty());
    docs.body = parts.join("\n\n");
    docs
}

/// `{type} rest`: the type and the rest.
fn braced_type(text: &str) -> (Option<String>, String) {
    if let Some(rest) = text.strip_prefix('{')
        && let Some((ty, rest)) = rest.split_once('}')
    {
        return (Some(ty.trim().to_owned()), rest.trim().to_owned());
    }
    (None, text.trim().to_owned())
}

/// A parameter tag's text: `[in] name text`, `{type} name text`, `name -
/// text` (TSDoc) or `name text`.
fn tag_entry(text: &str) -> Entry {
    let text = text.trim_start();
    // Doxygen's direction (`[in]`, `[out]`, `[in,out]`); JSDoc's `[name]`
    // (an optional parameter) stays.
    let text = match text.strip_prefix('[').and_then(|r| r.split_once(']')) {
        Some((direction, rest))
            if matches!(direction.replace(' ', "").as_str(), "in" | "out" | "in,out") =>
        {
            rest.trim_start()
        }
        _ => text,
    };
    let (ty, rest) = braced_type(text);
    let (name, rest) = rest
        .split_once(char::is_whitespace)
        .map_or((rest.as_str(), ""), |(n, r)| (n, r));
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("- ").unwrap_or(rest);
    Entry {
        name: Some(name.trim_matches(['[', ']']).to_owned()),
        ty,
        text: rest.trim().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn entry(name: Option<&str>, ty: Option<&str>, text: &str) -> Entry {
        Entry {
            name: name.map(str::to_owned),
            ty: ty.map(str::to_owned),
            text: text.to_owned(),
        }
    }

    #[test]
    fn google_style() {
        let docs = parse(
            "Fetch rows.\n\n    More about it.\n\n    Args:\n        table (str): The table,\n            by name.\n        keys: Keys.\n\n    Returns:\n        dict: Rows by key.\n\n    Raises:\n        IOError: When it fails.\n\n    Example:\n        >>> fetch('t')\n\n    Note:\n        Slow.\n",
            DocStyle::Auto,
        );
        assert_eq!(docs.body, "Fetch rows.\n\nMore about it.");
        assert_eq!(
            docs.params,
            [
                entry(Some("table"), Some("str"), "The table, by name."),
                entry(Some("keys"), None, "Keys.")
            ]
        );
        assert_eq!(
            docs.returns,
            Some(entry(None, Some("dict"), "Rows by key."))
        );
        assert_eq!(
            docs.raises,
            [entry(None, Some("IOError"), "When it fails.")]
        );
        assert_eq!(docs.examples, [">>> fetch('t')"]);
        assert_eq!(docs.sections, [("Note".to_owned(), "Slow.".to_owned())]);
    }

    #[test]
    fn numpy_style() {
        let docs = parse(
            "Sum things.\n\nParameters\n----------\nx : int\n    The first.\ny\n    The second.\n\nReturns\n-------\nint\n    The sum.\n\nSee Also\n--------\nproduct : Multiply.\n",
            DocStyle::Auto,
        );
        assert_eq!(docs.body, "Sum things.");
        assert_eq!(
            docs.params,
            [
                entry(Some("x"), Some("int"), "The first."),
                entry(Some("y"), None, "The second.")
            ]
        );
        assert_eq!(docs.returns, Some(entry(None, Some("int"), "The sum.")));
        assert_eq!(docs.see_also, ["product"]);
    }

    #[test]
    fn sphinx_style() {
        let docs = parse(
            "Open it.\n\n:param str path: Where.\n:param mode: How,\n   in detail.\n:type mode: int\n:returns: A handle.\n:rtype: File\n:raises OSError: If missing.\n",
            DocStyle::Auto,
        );
        assert_eq!(docs.body, "Open it.");
        assert_eq!(
            docs.params,
            [
                entry(Some("path"), Some("str"), "Where."),
                entry(Some("mode"), Some("int"), "How, in detail.")
            ]
        );
        assert_eq!(docs.returns, Some(entry(None, Some("File"), "A handle.")));
        assert_eq!(docs.raises, [entry(None, Some("OSError"), "If missing.")]);
    }

    #[test]
    fn tag_styles() {
        let docs = parse(
            "Parses it.\n\n@param {string} text - The input.\n@param [opts] Options,\n  more.\n@returns {Tree} The tree.\n@throws {SyntaxError} On bad input.\n@deprecated Use `parse2`.\n@since 1.2\n@example\n```ts\nparse('x')\n```\n",
            DocStyle::Auto,
        );
        assert_eq!(docs.body, "Parses it.");
        assert_eq!(
            docs.params,
            [
                entry(Some("text"), Some("string"), "The input."),
                entry(Some("opts"), None, "Options, more.")
            ]
        );
        assert_eq!(docs.returns, Some(entry(None, Some("Tree"), "The tree.")));
        assert_eq!(
            docs.raises,
            [entry(None, Some("SyntaxError"), "On bad input.")]
        );
        assert_eq!(docs.deprecated.as_deref(), Some("Use `parse2`."));
        assert_eq!(docs.since.as_deref(), Some("1.2"));
        assert_eq!(docs.examples, ["```ts\nparse('x')\n```"]);

        let doxygen = parse(
            "\\brief Adds.\n\\param[in] a First.\n\\tparam T The type.\n\\return The sum.\n",
            DocStyle::Auto,
        );
        assert_eq!(doxygen.body, "Adds.");
        assert_eq!(doxygen.params, [entry(Some("a"), None, "First.")]);
        assert_eq!(doxygen.type_params, [entry(Some("T"), None, "The type.")]);
        assert_eq!(doxygen.returns, Some(entry(None, None, "The sum.")));
    }

    #[test]
    fn plain_text_is_all_body() {
        let docs = parse("Just text.\n\nArgs are fine here.", DocStyle::Auto);
        assert_eq!(detect("Just text."), DocStyle::Plain);
        assert_eq!(docs.body, "Just text.\n\nArgs are fine here.");
        assert_eq!(
            parse("Text.\n\nArgs:\n    x: y", DocStyle::Plain).body,
            "Text.\n\nArgs:\n    x: y"
        );
    }
}
