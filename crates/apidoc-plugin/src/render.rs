//! Items to document nodes: generated pages, and items placed in
//! hand-written pages.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use lsdocs_apidoc::docstring::{self, Docs, Entry};
use lsdocs_apidoc::{Index, Item, anchor};
use lsdocs_plugin_sdk::doctree::{
    Admonition, Anchor, CodeBlock, Custom, DefinitionDescription, DefinitionList, DefinitionTerm,
    Document, Emphasis, Heading, InlineCode, Link, LinkTarget, List, ListItem, Node, Paragraph,
    Section, Strong, text,
};
use lsdocs_plugin_sdk::protocol::Diagnostic;
use lsdocs_plugin_sdk::{Context, relative_path};
use serde_json::json;

use crate::{Language, Options, domain, warning};

/// Where each item is documented.
#[derive(Debug, Default)]
pub(crate) struct Pages {
    /// Every documented item and the page it is on.
    pub placed: Vec<(String, Item)>,
}

/// The page of the module at `path`.
pub(crate) fn module_page(output: &str, path: &[String]) -> String {
    format!("{output}/{}.md", path.join("."))
}

/// The page listing the modules.
pub(crate) fn index_page(output: &str) -> String {
    format!("{output}/index.md")
}

/// Places every item: a module on its own page, anything else on its
/// module's page (or the index page, outside any module).
pub(crate) fn pages(items: &[Item], output: &str) -> Pages {
    fn place(item: &Item, page: &str, output: &str, out: &mut Pages) {
        let page = if item.kind.is_module() {
            module_page(output, &item.path)
        } else {
            page.to_owned()
        };
        out.placed.push((page.clone(), item.clone()));
        for member in &item.members {
            place(member, &page, output, out);
        }
    }
    let mut out = Pages::default();
    for item in items {
        place(item, &index_page(output), output, &mut out);
    }
    out
}

/// Renders items, parsing doc comments through the host.
pub(crate) struct Renderer<'a, L: Language> {
    pub language: &'a L,
    format: String,
    style: docstring::DocStyle,
    ctx: &'a Context,
    pub diagnostics: Mutex<Vec<Diagnostic>>,
}

/// Parsed doc comment pieces, by text.
type Parsed = BTreeMap<String, Vec<Node>>;

impl<'a, L: Language> Renderer<'a, L> {
    pub fn new(language: &'a L, options: &Options, ctx: &'a Context) -> Self {
        Self {
            format: options
                .doc_format
                .clone()
                .unwrap_or_else(|| language.doc_format().to_owned()),
            style: options.doc_style.unwrap_or_else(|| language.doc_style()),
            language,
            ctx,
            diagnostics: Mutex::default(),
        }
    }

    fn warn(&self, message: String) {
        self.diagnostics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(warning(None, message));
    }

    /// Takes the problems found so far.
    pub fn take_diagnostics(&self) -> Vec<Diagnostic> {
        std::mem::take(
            &mut self
                .diagnostics
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    fn docs(&self, item: &Item) -> Option<Docs> {
        item.docs
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .map(|d| docstring::parse(d, self.style))
    }

    /// Parses the doc comment pieces of `items` (and their members, if
    /// `members`), as part of `page`.
    async fn parse_docs(&self, items: &[&Item], members: bool, page: &str) -> Parsed {
        let mut texts = Vec::new();
        for item in items {
            let all = if members { item.walk() } else { vec![*item] };
            for item in all {
                if let Some(docs) = self.docs(item) {
                    texts.extend(pieces(&docs));
                }
            }
        }
        let mut parsed = Parsed::new();
        for text in texts {
            if text.trim().is_empty() || parsed.contains_key(&text) {
                continue;
            }
            let nodes = match self.ctx.parse(&self.format, page, &text).await {
                Ok(result) => {
                    for d in result.diagnostics {
                        self.warn(format!("in a doc comment: {}", d.message));
                    }
                    result.document.children
                }
                Err(e) => {
                    self.warn(format!("cannot parse a doc comment: {e}"));
                    vec![Node::Paragraph(Paragraph {
                        span: None,
                        children: vec![text_node(&text)],
                    })]
                }
            };
            parsed.insert(text, nodes);
        }
        parsed
    }

    /// The generated pages: one per module, and the index page.
    pub async fn documents(&self, items: &[Item], output: &str, index: &Index) -> Vec<Document> {
        let mut documents = Vec::new();
        let modules: Vec<&Item> = items
            .iter()
            .flat_map(Item::walk)
            .filter(|i| i.kind.is_module())
            .collect();
        let loose: Vec<&Item> = items.iter().filter(|i| !i.kind.is_module()).collect();
        let sep = self.language.separator();
        for module in &modules {
            let page = module_page(output, &module.path);
            let parsed = self.parse_docs(&[module], true, &page).await;
            let mut doc = Document::new(&page);
            doc.metadata
                .insert("title".into(), json!(module.qualified(sep)));
            let mut node = self.item(module, 1, &page, &parsed, true, output, index);
            if let Node::Section(s) = &mut node
                && let Some(Node::Heading(h)) = s.children.first_mut()
            {
                h.children = vec![text(module.qualified(sep))];
            }
            doc.children = vec![node];
            documents.push(doc);
        }
        // The index page: the modules, then items outside any module.
        let page = index_page(output);
        let parsed = self.parse_docs(&loose, true, &page).await;
        let title = format!("{} API", self.language.name());
        let mut children = vec![Node::Heading(Heading {
            span: None,
            level: 1,
            id: None,
            children: vec![text(&title)],
        })];
        if !modules.is_empty() {
            let entries = modules
                .iter()
                .map(|m| {
                    let target = module_page(output, &m.path);
                    list_item(vec![link(
                        &page,
                        &target,
                        None,
                        vec![code(&m.qualified(sep))],
                    )])
                })
                .collect();
            children.push(list(entries));
        }
        for item in loose {
            children.push(self.item(item, 2, &page, &parsed, true, output, index));
        }
        let mut doc = Document::new(&page);
        doc.metadata.insert("title".into(), json!(title));
        doc.children = vec![Node::Section(Section {
            span: None,
            id: None,
            children,
        })];
        documents.insert(0, doc);
        documents
    }

    /// `item` for a hand-written page (`<domain>:auto`): a block with its
    /// signature and documentation, members included if `members`.
    pub async fn auto(&self, item: &Item, members: bool, page: &str, index: &Index) -> Vec<Node> {
        let parsed = self.parse_docs(&[item], members, page).await;
        vec![self.block(item, page, &parsed, members, index)]
    }

    /// A block for `item` (no heading): an anchor, the signature, the
    /// documentation and, if `members`, its members' blocks.
    fn block(
        &self,
        item: &Item,
        page: &str,
        parsed: &Parsed,
        members: bool,
        index: &Index,
    ) -> Node {
        let id = anchor(self.language.domain(), &item.path);
        let mut children = vec![
            Node::Anchor(Anchor {
                span: None,
                id: id.clone(),
            }),
            self.signature(item),
        ];
        children.extend(self.documentation(item, page, parsed, index));
        if members {
            for member in &item.members {
                if member.kind.is_module() {
                    continue;
                }
                children.push(self.block(member, page, parsed, true, index));
            }
        }
        api_item(item, children)
    }

    /// A section for `item` at heading `level`, with its members' sections
    /// (modules among them as links to their pages).
    #[allow(clippy::too_many_arguments)]
    fn item(
        &self,
        item: &Item,
        level: u8,
        page: &str,
        parsed: &Parsed,
        members: bool,
        output: &str,
        index: &Index,
    ) -> Node {
        let mut heading = vec![
            Node::Custom(Custom {
                span: None,
                kind: "api-kind".into(),
                attrs: [
                    ("inline".to_owned(), json!(true)),
                    ("html_tag".to_owned(), json!("em")),
                ]
                .into(),
                children: vec![text(item.kind.label())],
            }),
            text(" "),
            code(item.name()),
        ];
        if item.kind.is_module() {
            heading = vec![text(item.qualified(self.language.separator()))];
        }
        let mut children = vec![Node::Heading(Heading {
            span: None,
            level: level.min(6),
            id: None,
            children: heading,
        })];
        if !item.kind.is_module() {
            children.push(self.signature(item));
        }
        children.extend(self.documentation(item, page, parsed, index));
        if members {
            let submodules: Vec<&Item> =
                item.members.iter().filter(|m| m.kind.is_module()).collect();
            if !submodules.is_empty() {
                children.push(field_title("Submodules"));
                children.push(list(
                    submodules
                        .iter()
                        .map(|m| {
                            let target = module_page(output, &m.path);
                            list_item(vec![link(
                                page,
                                &target,
                                None,
                                vec![code(&m.qualified(self.language.separator()))],
                            )])
                        })
                        .collect(),
                ));
            }
            for member in item.members.iter().filter(|m| !m.kind.is_module()) {
                children.push(self.item(
                    member,
                    level.saturating_add(1),
                    page,
                    parsed,
                    true,
                    output,
                    index,
                ));
            }
        }
        Node::Section(Section {
            span: None,
            id: Some(anchor(self.language.domain(), &item.path)),
            children,
        })
    }

    fn signature(&self, item: &Item) -> Node {
        Node::CodeBlock(CodeBlock {
            span: None,
            language: Some(self.language.code_language().to_owned()),
            info: None,
            value: self.language.signature(item),
        })
    }

    /// An item's documentation: its doc comment's parts, with references
    /// in them resolved from the item.
    fn documentation(&self, item: &Item, page: &str, parsed: &Parsed, index: &Index) -> Vec<Node> {
        let Some(docs) = self.docs(item) else {
            return Vec::new();
        };
        let get = |text: &str| parsed.get(text).cloned().unwrap_or_default();
        let mut out = Vec::new();
        if let Some(reason) = &docs.deprecated {
            out.push(Node::Admonition(Admonition {
                span: None,
                kind: "deprecated".into(),
                title: Some("Deprecated".into()),
                children: get(reason),
            }));
        }
        out.extend(get(&docs.body));
        if let Some(since) = &docs.since {
            out.push(Node::Paragraph(Paragraph {
                span: None,
                children: vec![Node::Emphasis(Emphasis {
                    span: None,
                    children: vec![text(format!("Added in version {since}."))],
                })],
            }));
        }
        let fields: [(&str, &[Entry]); 4] = [
            ("Parameters", &docs.params),
            ("Type parameters", &docs.type_params),
            ("Raises", &docs.raises),
            ("Attributes", &docs.attributes),
        ];
        for (title, entries) in fields.iter().take(2) {
            out.extend(entry_list(title, entries, &get));
        }
        for (title, entry) in [("Returns", &docs.returns), ("Yields", &docs.yields)] {
            if let Some(entry) = entry {
                out.extend(entry_list(title, std::slice::from_ref(entry), &get));
            }
        }
        for (title, entries) in fields.iter().skip(2) {
            out.extend(entry_list(title, entries, &get));
        }
        if !docs.examples.is_empty() {
            out.push(field_title(if docs.examples.len() > 1 {
                "Examples"
            } else {
                "Example"
            }));
            for example in &docs.examples {
                out.extend(get(example));
            }
        }
        for (title, body) in &docs.sections {
            let kind = title.to_lowercase();
            if matches!(
                kind.as_str(),
                "note"
                    | "notes"
                    | "warning"
                    | "warnings"
                    | "tip"
                    | "important"
                    | "caution"
                    | "danger"
                    | "attention"
                    | "hint"
                    | "error"
                    | "todo"
            ) {
                out.push(Node::Admonition(Admonition {
                    span: None,
                    kind: kind.trim_end_matches('s').to_owned(),
                    title: Some(title.clone()),
                    children: get(body),
                }));
            } else {
                out.push(field_title(title));
                out.extend(get(body));
            }
        }
        if !docs.see_also.is_empty() {
            out.push(field_title("See also"));
            let mut inlines = Vec::new();
            for (i, name) in docs.see_also.iter().enumerate() {
                if i > 0 {
                    inlines.push(text(", "));
                }
                inlines.push(Node::Custom(Custom {
                    span: None,
                    kind: "role".into(),
                    attrs: [
                        (
                            "name".to_owned(),
                            json!(format!("{}:obj", self.language.domain())),
                        ),
                        ("text".to_owned(), json!(name)),
                    ]
                    .into(),
                    children: vec![text(name)],
                }));
            }
            out.push(Node::Paragraph(Paragraph {
                span: None,
                children: inlines,
            }));
        }
        let mut problems = Vec::new();
        domain::resolve_roles(
            self.language,
            index,
            &mut out,
            page,
            &item.path,
            &mut problems,
        );
        self.diagnostics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(problems);
        out
    }
}

/// The pieces of a doc comment that are markup to parse.
fn pieces(docs: &Docs) -> Vec<String> {
    let mut out = vec![docs.body.clone()];
    for entry in docs
        .params
        .iter()
        .chain(&docs.type_params)
        .chain(&docs.raises)
        .chain(&docs.attributes)
        .chain(&docs.returns)
        .chain(&docs.yields)
    {
        out.push(entry.text.clone());
    }
    out.extend(docs.examples.iter().cloned());
    out.extend(docs.sections.iter().map(|(_, t)| t.clone()));
    out.extend(docs.deprecated.iter().cloned());
    out
}

/// A titled list of entries: `name (type)` terms and their descriptions.
fn entry_list(title: &str, entries: &[Entry], get: &impl Fn(&str) -> Vec<Node>) -> Vec<Node> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut items = Vec::new();
    for entry in entries {
        let mut term = Vec::new();
        if let Some(name) = &entry.name {
            term.push(Node::Strong(Strong {
                span: None,
                children: vec![text(name)],
            }));
        }
        if let Some(ty) = &entry.ty {
            if !term.is_empty() {
                term.push(text(" ("));
                term.push(code(ty));
                term.push(text(")"));
            } else {
                term.push(code(ty));
            }
        }
        if term.is_empty() {
            // A return value with only a description.
            items.push(Node::DefinitionTerm(DefinitionTerm {
                span: None,
                children: vec![text("")],
            }));
        } else {
            items.push(Node::DefinitionTerm(DefinitionTerm {
                span: None,
                children: term,
            }));
        }
        items.push(Node::DefinitionDescription(DefinitionDescription {
            span: None,
            children: get(&entry.text),
        }));
    }
    vec![
        field_title(title),
        Node::DefinitionList(DefinitionList {
            span: None,
            children: items,
        }),
    ]
}

/// A custom block holding a hand-placed item.
pub(crate) fn api_item(item: &Item, children: Vec<Node>) -> Node {
    Node::Custom(Custom {
        span: None,
        kind: "api-item".into(),
        attrs: [
            ("item_kind".to_owned(), json!(item.kind.label())),
            ("scope".to_owned(), json!(item.path)),
        ]
        .into(),
        children,
    })
}

fn field_title(title: &str) -> Node {
    Node::Paragraph(Paragraph {
        span: None,
        children: vec![Node::Strong(Strong {
            span: None,
            children: vec![text(title)],
        })],
    })
}

pub(crate) fn code(value: &str) -> Node {
    Node::InlineCode(InlineCode {
        span: None,
        value: value.to_owned(),
    })
}

fn text_node(value: &str) -> Node {
    text(value)
}

fn list(items: Vec<Node>) -> Node {
    Node::List(List {
        span: None,
        ordered: false,
        start: None,
        children: items,
    })
}

fn list_item(inlines: Vec<Node>) -> Node {
    Node::ListItem(ListItem {
        span: None,
        checked: None,
        children: vec![Node::Paragraph(Paragraph {
            span: None,
            children: inlines,
        })],
    })
}

/// A link from the page `from` to `anchor` on the page `to`.
pub(crate) fn link(from: &str, to: &str, anchor: Option<&str>, children: Vec<Node>) -> Node {
    let mut url = if from == to {
        String::new()
    } else {
        relative_path(from, to)
    };
    if let Some(anchor) = anchor {
        url.push('#');
        url.push_str(anchor);
    }
    Node::Link(Link {
        span: None,
        url,
        title: None,
        target: Some(LinkTarget {
            document: Some(to.to_owned()),
            anchor: anchor.map(str::to_owned),
            label: None,
        }),
        children,
    })
}
