//! The API model: items, their signatures and where they are.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What kind of thing an [`Item`] is. The kinds are shared by every
/// language; a language with something else uses [`ItemKind::Other`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// A module (a Python module, a Rust module, a TypeScript file).
    Module,
    /// A package of modules.
    Package,
    /// A namespace (C++, TypeScript).
    Namespace,
    /// A class.
    Class,
    /// A struct.
    Struct,
    /// A union.
    Union,
    /// An interface.
    Interface,
    /// A trait.
    Trait,
    /// An enumeration.
    Enum,
    /// A variant or enumerator of an enumeration.
    Variant,
    /// A free function.
    Function,
    /// A function that belongs to a type.
    Method,
    /// A constructor.
    Constructor,
    /// A field of a struct or class.
    Field,
    /// A property (computed or declared).
    Property,
    /// A constant.
    Constant,
    /// A variable.
    Variable,
    /// A type alias.
    TypeAlias,
    /// A macro.
    Macro,
    /// Anything else, by the language's own name for it.
    Other(String),
}

impl ItemKind {
    /// The kind's name in lower case, as shown to readers: `class`,
    /// `type alias`, or an [`ItemKind::Other`]'s own name.
    pub fn label(&self) -> &str {
        match self {
            ItemKind::Module => "module",
            ItemKind::Package => "package",
            ItemKind::Namespace => "namespace",
            ItemKind::Class => "class",
            ItemKind::Struct => "struct",
            ItemKind::Union => "union",
            ItemKind::Interface => "interface",
            ItemKind::Trait => "trait",
            ItemKind::Enum => "enum",
            ItemKind::Variant => "variant",
            ItemKind::Function => "function",
            ItemKind::Method => "method",
            ItemKind::Constructor => "constructor",
            ItemKind::Field => "field",
            ItemKind::Property => "property",
            ItemKind::Constant => "constant",
            ItemKind::Variable => "variable",
            ItemKind::TypeAlias => "type alias",
            ItemKind::Macro => "macro",
            ItemKind::Other(name) => name,
        }
    }

    /// Whether items of this kind hold others and get a page of their own
    /// when pages are made per module.
    pub fn is_module(&self) -> bool {
        matches!(
            self,
            ItemKind::Module | ItemKind::Package | ItemKind::Namespace
        )
    }

    /// Whether this is a kind of type (class, struct, enum, ...).
    pub fn is_type(&self) -> bool {
        matches!(
            self,
            ItemKind::Class
                | ItemKind::Struct
                | ItemKind::Union
                | ItemKind::Interface
                | ItemKind::Trait
                | ItemKind::Enum
                | ItemKind::TypeAlias
        )
    }

    /// Whether this is a kind of function.
    pub fn is_callable(&self) -> bool {
        matches!(
            self,
            ItemKind::Function | ItemKind::Method | ItemKind::Constructor | ItemKind::Macro
        )
    }
}

/// Who can use an item.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Anyone.
    #[default]
    Public,
    /// Subclasses (and the like).
    Protected,
    /// Its own package or crate.
    Internal,
    /// Only where it is defined.
    Private,
}

/// Where an item is defined.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// The file, as the extractor names it (usually relative to the
    /// project).
    pub file: String,
    /// The line, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// How a parameter is passed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    /// An ordinary parameter.
    #[default]
    Normal,
    /// The receiver (`self`, `this`).
    Receiver,
    /// Only by position (Python's before `/`).
    PositionalOnly,
    /// Only by name (Python's after `*`).
    KeywordOnly,
    /// Any number of positional arguments (`*args`, `...rest`).
    Variadic,
    /// Any number of named arguments (`**kwargs`).
    KeywordVariadic,
}

/// A parameter of a callable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    /// The name.
    pub name: String,
    /// The type, as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// The default value, as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// How it is passed.
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ParamKind,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// A callable's (or a generic type's) signature. Types are kept as
/// written; languages format them ([`Signature::text`] can hold a
/// signature exactly as the source has it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    /// Generic parameters, each as written (`T: Clone`, `K extends string`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generics: Vec<String>,
    /// The parameters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
    /// The return type, as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<String>,
    /// Modifiers such as `async`, `static`, `const`, `unsafe`, `abstract`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
    /// The whole signature as the source writes it, if the extractor
    /// prefers that to formatting the parts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Something documented: a module, a type, a function, a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// What it is.
    pub kind: ItemKind,
    /// Its qualified name, outermost first: `["pkg", "mod", "Class",
    /// "method"]`. The last part is its own name.
    pub path: Vec<String>,
    /// Its signature, for callables and generic types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Signature>,
    /// The type of a field, property, constant, variable or alias (for an
    /// alias, what it stands for), as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// The value of a constant or variant, as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Types it extends or implements, as written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<String>,
    /// Who can use it.
    #[serde(default, skip_serializing_if = "is_default")]
    pub visibility: Visibility,
    /// Its doc comment, comment markers removed, in the language's doc
    /// markup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// Where it is defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// For a re-export, the qualified name of the item it re-exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reexport_of: Option<Vec<String>>,
    /// Language-specific facts (decorators, attributes, `abstract`),
    /// shown as they are.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
    /// Items inside it, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<Item>,
}

impl Item {
    /// An item of `kind` at `path`, with nothing else known yet.
    pub fn new(kind: ItemKind, path: Vec<String>) -> Self {
        Self {
            kind,
            path,
            signature: None,
            ty: None,
            value: None,
            bases: Vec::new(),
            visibility: Visibility::Public,
            docs: None,
            location: None,
            reexport_of: None,
            extra: BTreeMap::new(),
            members: Vec::new(),
        }
    }

    /// Its own name: the last part of its path.
    pub fn name(&self) -> &str {
        self.path.last().map_or("", String::as_str)
    }

    /// Its qualified name, joined with `separator` (`.` or `::`).
    pub fn qualified(&self, separator: &str) -> String {
        self.path.join(separator)
    }

    /// This item and everything inside it, depth first, parents first.
    pub fn walk(&self) -> Vec<&Item> {
        let mut out = vec![self];
        for member in &self.members {
            out.extend(member.walk());
        }
        out
    }

    /// Drops members `keep` says no to, at every depth.
    pub fn retain(&mut self, keep: &impl Fn(&Item) -> bool) {
        self.members.retain(|m| keep(m));
        for member in &mut self.members {
            member.retain(keep);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn path(s: &str) -> Vec<String> {
        s.split('.').map(str::to_owned).collect()
    }

    #[test]
    fn items_serialize_compactly() {
        let mut item = Item::new(ItemKind::Function, path("pkg.f"));
        item.signature = Some(Signature {
            params: vec![Param {
                name: "x".into(),
                ty: Some("int".into()),
                ..Param::default()
            }],
            returns: Some("str".into()),
            ..Signature::default()
        });
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "function",
                "path": ["pkg", "f"],
                "signature": {"params": [{"name": "x", "ty": "int"}], "returns": "str"},
            })
        );
        assert_eq!(serde_json::from_value::<Item>(json).unwrap(), item);
        let other: ItemKind =
            serde_json::from_value(serde_json::json!({"other": "decorator"})).unwrap();
        assert_eq!(other.label(), "decorator");
    }

    #[test]
    fn walking_and_filtering() {
        let mut module = Item::new(ItemKind::Module, path("m"));
        let mut class = Item::new(ItemKind::Class, path("m.C"));
        let mut private = Item::new(ItemKind::Method, path("m.C._p"));
        private.visibility = Visibility::Private;
        class.members.push(private);
        class
            .members
            .push(Item::new(ItemKind::Method, path("m.C.p")));
        module.members.push(class);
        let names: Vec<_> = module.walk().iter().map(|i| i.qualified(".")).collect();
        assert_eq!(names, ["m", "m.C", "m.C._p", "m.C.p"]);
        module.retain(&|i| i.visibility == Visibility::Public);
        assert_eq!(module.walk().len(), 3);
        assert_eq!(module.members[0].name(), "C");
    }
}
