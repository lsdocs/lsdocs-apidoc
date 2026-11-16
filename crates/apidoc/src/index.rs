//! Finding items by name: every documented item of a site (generated or
//! written by hand), where it is, and how a reference resolves.

use serde::{Deserialize, Serialize};

use crate::model::ItemKind;

/// The anchor id of the item at `path` in `domain`: `py.pkg.mod.Class`.
/// Ids keep their case, since names differ only by case in many languages.
pub fn anchor(domain: &str, path: &[String]) -> String {
    let mut id = String::from(domain);
    for part in path {
        id.push('.');
        id.push_str(part);
    }
    id
}

/// Splits a reference into the parts of a qualified name, at `.` and
/// `::` (`a::b.c` is `a`, `b`, `c`).
pub fn split_name(name: &str) -> Vec<String> {
    name.split("::")
        .flat_map(|p| p.split('.'))
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

/// An item's entry in the [`Index`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The domain (language) it belongs to: `py`, `rs`, `cpp`, ...
    pub domain: String,
    /// Its qualified name.
    pub path: Vec<String>,
    /// What it is.
    pub kind: ItemKind,
    /// The page it is documented on (a source path).
    pub page: String,
    /// Its anchor on that page.
    pub anchor: String,
}

/// Every item known, to resolve references.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    entries: Vec<Entry>,
}

impl Index {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry. The first entry for a name wins when two have the
    /// same domain, path and kind.
    pub fn add(&mut self, entry: Entry) {
        let taken = self
            .entries
            .iter()
            .any(|e| e.domain == entry.domain && e.path == entry.path && e.kind == entry.kind);
        if !taken {
            self.entries.push(entry);
        }
    }

    /// Every entry, in the order added.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Resolves `name` (`Class.method`, `pkg::f`) in `domain`, seen from
    /// `scope` (the qualified name of the module or type the reference is
    /// in): first relative to the scope and each scope around it, then as
    /// the end of exactly one qualified name. `kinds`, if given, limits the
    /// kinds that match (`:py:func:` takes functions and methods).
    pub fn resolve(
        &self,
        domain: &str,
        name: &str,
        scope: &[String],
        kinds: Option<&[ItemKind]>,
    ) -> Resolved<'_> {
        let parts = split_name(name);
        if parts.is_empty() {
            return Resolved::Missing;
        }
        let candidates = || {
            self.entries
                .iter()
                .filter(move |e| e.domain == domain && kinds.is_none_or(|k| k.contains(&e.kind)))
        };
        for depth in (0..=scope.len()).rev() {
            let mut full = scope[..depth].to_vec();
            full.extend(parts.iter().cloned());
            if let Some(entry) = candidates().find(|e| e.path == full) {
                return Resolved::Found(entry);
            }
        }
        let matching: Vec<&Entry> = candidates()
            .filter(|e| e.path.len() >= parts.len() && e.path.ends_with(&parts))
            .collect();
        match matching.as_slice() {
            [] => Resolved::Missing,
            [one] => Resolved::Found(one),
            many => Resolved::Ambiguous(many.to_vec()),
        }
    }
}

/// What a reference resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved<'a> {
    /// One item.
    Found(&'a Entry),
    /// Several items end with the name, none in scope.
    Ambiguous(Vec<&'a Entry>),
    /// Nothing.
    Missing,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, kind: ItemKind) -> Entry {
        let path = split_name(path);
        Entry {
            domain: "py".into(),
            anchor: anchor("py", &path),
            path,
            kind,
            page: "api/pkg.md".into(),
        }
    }

    fn index() -> Index {
        let mut index = Index::new();
        index.add(entry("pkg.mod.Class", ItemKind::Class));
        index.add(entry("pkg.mod.Class.run", ItemKind::Method));
        index.add(entry("pkg.mod.run", ItemKind::Function));
        index.add(entry("pkg.other.Class", ItemKind::Class));
        index
    }

    fn found(r: Resolved<'_>) -> String {
        match r {
            Resolved::Found(e) => e.anchor.clone(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn names_resolve_in_scope_then_by_suffix() {
        let index = index();
        let scope = split_name("pkg.mod.Class");
        // In the class, `run` is its method; in the module, the function.
        assert_eq!(
            found(index.resolve("py", "run", &scope, None)),
            "py.pkg.mod.Class.run"
        );
        assert_eq!(
            found(index.resolve("py", "run", &split_name("pkg.mod"), None)),
            "py.pkg.mod.run"
        );
        let functions = [ItemKind::Function];
        assert_eq!(
            found(index.resolve("py", "run", &scope, Some(&functions))),
            "py.pkg.mod.run"
        );
        assert_eq!(
            found(index.resolve("py", "pkg::other::Class", &[], None)),
            "py.pkg.other.Class"
        );
        assert!(
            matches!(index.resolve("py", "Class", &[], None), Resolved::Ambiguous(v) if v.len() == 2)
        );
        assert_eq!(index.resolve("rs", "run", &[], None), Resolved::Missing);
        assert_eq!(index.resolve("py", "", &[], None), Resolved::Missing);
    }
}
