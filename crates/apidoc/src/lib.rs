//! A language-neutral model of API documentation.
//!
//! Extractors for each language (Python, TypeScript, C/C++, Rust) turn
//! source code into [`Item`]s: modules, types, functions and the rest, with
//! their signatures as written and their doc comments. This crate holds
//! what they share:
//!
//! - [`model`]: the items, signatures, parameters and locations.
//! - [`docstring`]: the sections doc comment conventions use (Google,
//!   NumPy, Sphinx field lists, Javadoc / TSDoc / Doxygen tags), split out
//!   of a comment, its text left in its markup.
//! - [`index`]: anchors for items, and resolving a name in a scope.
//!
//! It depends on nothing from lsdocs: the lsdocs plugins that generate
//! pages from it (`lsdocs-apidoc-plugin`) are adapters over it.

pub mod docstring;
pub mod index;
pub mod model;

pub use docstring::{DocStyle, Docs};
pub use index::{Index, Resolved, anchor};
pub use model::{Item, ItemKind, Location, Param, ParamKind, Signature, Visibility};
