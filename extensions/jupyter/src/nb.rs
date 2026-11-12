//! A notebook as its file has it: nbformat 4's JSON, kept whole as a
//! `Value` so what Fenix doesn't know of (metadata, widgets' state,
//! attachments) is written back as it was, and read through cells.
//!
//! It's written as Jupyter writes it -- keys sorted, one space a level,
//! a newline at the end, a cell's source as a list of lines -- so a
//! notebook saved here and in Jupyter differs only where it changed.

use serde::Serialize;
use serde_json::{json, Map, Value};

/// What a cell is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Code,
    Markdown,
    Raw,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Code => "code",
            Kind::Markdown => "markdown",
            Kind::Raw => "raw",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notebook {
    root: Value,
}

/// A multiline string as nbformat has it: a string, or a list of them
/// to be joined.
pub fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// `text` as a list of lines, each with its newline but the last, as
/// Jupyter writes sources and outputs.
pub fn lines_of(text: &str) -> Value {
    Value::Array(text.split_inclusive('\n').map(|l| Value::String(l.to_string())).collect())
}

impl Notebook {
    /// The notebook in `text`. An empty file is a new notebook.
    pub fn parse(text: &str) -> Result<Notebook, String> {
        if text.trim().is_empty() {
            return Ok(Notebook::empty());
        }
        let root: Value = serde_json::from_str(text).map_err(|e| format!("not a notebook: {e}"))?;
        let major = root.get("nbformat").and_then(Value::as_u64).unwrap_or(0);
        if major < 4 {
            return Err(format!("nbformat {major}: Fenix reads nbformat 4 (jupyter nbconvert --to notebook --inplace upgrades it)"));
        }
        if !root.get("cells").is_some_and(Value::is_array) {
            return Err("not a notebook: it has no cells".into());
        }
        Ok(Notebook { root })
    }

    /// A notebook of one empty code cell, for Python.
    pub fn empty() -> Notebook {
        let mut nb = Notebook {
            root: json!({
                "cells": [],
                "metadata": {
                    "kernelspec": {"display_name": "Python 3 (ipykernel)", "language": "python", "name": "python3"},
                    "language_info": {"name": "python"}
                },
                "nbformat": 4,
                "nbformat_minor": 5
            }),
        };
        nb.insert(0, Kind::Code);
        nb
    }

    /// Its text, as Jupyter writes it.
    pub fn to_text(&self) -> String {
        let mut out = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
        let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
        sorted(&self.root).serialize(&mut ser).expect("a Value always serializes");
        let mut text = String::from_utf8(out).expect("serde_json writes UTF-8");
        text.push('\n');
        text
    }

    fn cells(&self) -> &[Value] {
        self.root["cells"].as_array().map(Vec::as_slice).unwrap_or(&[])
    }

    fn cells_mut(&mut self) -> &mut Vec<Value> {
        if !self.root["cells"].is_array() {
            self.root["cells"] = json!([]);
        }
        self.root["cells"].as_array_mut().expect("just made an array")
    }

    fn cell_mut(&mut self, i: usize) -> Option<&mut Map<String, Value>> {
        self.cells_mut().get_mut(i).and_then(Value::as_object_mut)
    }

    pub fn len(&self) -> usize {
        self.cells().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn kind(&self, i: usize) -> Kind {
        match self.cells().get(i).and_then(|c| c["cell_type"].as_str()) {
            Some("markdown") => Kind::Markdown,
            Some("raw") => Kind::Raw,
            _ => Kind::Code,
        }
    }

    pub fn source(&self, i: usize) -> String {
        self.cells().get(i).map(|c| text_of(&c["source"])).unwrap_or_default()
    }

    pub fn set_source(&mut self, i: usize, text: &str) {
        if let Some(c) = self.cell_mut(i) {
            c.insert("source".into(), lines_of(text));
        }
    }

    pub fn outputs(&self, i: usize) -> &[Value] {
        self.cells().get(i).and_then(|c| c["outputs"].as_array()).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Its execution count, the `[3]` beside it.
    pub fn count(&self, i: usize) -> Option<u64> {
        self.cells().get(i).and_then(|c| c["execution_count"].as_u64())
    }

    pub fn set_count(&mut self, i: usize, count: Option<u64>) {
        if self.kind(i) != Kind::Code {
            return;
        }
        if let Some(c) = self.cell_mut(i) {
            c.insert("execution_count".into(), count.map_or(Value::Null, Value::from));
        }
    }

    /// A code cell's outputs gone, and its count.
    pub fn clear_outputs(&mut self, i: usize) {
        if self.kind(i) != Kind::Code {
            return;
        }
        if let Some(c) = self.cell_mut(i) {
            c.insert("outputs".into(), json!([]));
            c.insert("execution_count".into(), Value::Null);
        }
    }

    /// An output that came for a code cell. Text to the same stream as
    /// the last output's joins it, as Jupyter keeps them.
    pub fn push_output(&mut self, i: usize, output: Value) {
        if self.kind(i) != Kind::Code {
            return;
        }
        let Some(c) = self.cell_mut(i) else { return };
        let outputs = c.entry("outputs").or_insert_with(|| json!([]));
        if !outputs.is_array() {
            *outputs = json!([]);
        }
        let list = outputs.as_array_mut().expect("just made an array");
        if output["output_type"] == "stream" {
            if let Some(last) = list.last_mut().filter(|l| l["output_type"] == "stream" && l["name"] == output["name"]) {
                let joined = text_of(&last["text"]) + &text_of(&output["text"]);
                last["text"] = lines_of(&joined);
                return;
            }
            let mut output = output;
            output["text"] = lines_of(&text_of(&output["text"]));
            list.push(output);
            return;
        }
        let mut output = output;
        // As nbformat writes them (`split_lines`): text kinds as lines.
        if let Some(data) = output.get_mut("data").and_then(Value::as_object_mut) {
            for (kind, v) in data.iter_mut() {
                if (kind.starts_with("text/") || kind == "application/javascript" || kind == "image/svg+xml") && v.is_string() {
                    *v = lines_of(&text_of(v));
                }
            }
        }
        list.push(output);
    }

    /// A cell made at `at`: an empty one of `kind`.
    pub fn insert(&mut self, at: usize, kind: Kind) {
        let mut cell = Map::new();
        cell.insert("cell_type".into(), kind.name().into());
        // nbformat 4.5's ids.
        if self.root["nbformat_minor"].as_u64().unwrap_or(0) >= 5 || self.root["nbformat"].as_u64().unwrap_or(4) > 4 {
            cell.insert("id".into(), self.new_id().into());
        }
        cell.insert("metadata".into(), json!({}));
        cell.insert("source".into(), json!([]));
        if kind == Kind::Code {
            cell.insert("execution_count".into(), Value::Null);
            cell.insert("outputs".into(), json!([]));
        }
        let at = at.min(self.len());
        self.cells_mut().insert(at, Value::Object(cell));
    }

    /// Cell `i` whole, to paste.
    pub fn cell(&self, i: usize) -> Option<Value> {
        self.cells().get(i).cloned()
    }

    /// A cell pasted at `at`: given an id of its own, if it has one.
    pub fn put(&mut self, at: usize, mut cell: Value) {
        if cell.get("id").is_some() {
            cell["id"] = self.new_id().into();
        }
        let at = at.min(self.len());
        self.cells_mut().insert(at, cell);
    }

    pub fn remove(&mut self, i: usize) -> Option<Value> {
        (i < self.len()).then(|| self.cells_mut().remove(i))
    }

    /// Cells `a` and `b` change places.
    pub fn swap(&mut self, a: usize, b: usize) {
        if a < self.len() && b < self.len() {
            self.cells_mut().swap(a, b);
        }
    }

    /// The cell made a `kind`: a code cell gets outputs, the others lose
    /// them.
    pub fn set_kind(&mut self, i: usize, kind: Kind) {
        if self.kind(i) == kind {
            return;
        }
        let Some(c) = self.cell_mut(i) else { return };
        c.insert("cell_type".into(), kind.name().into());
        if kind == Kind::Code {
            c.insert("execution_count".into(), Value::Null);
            c.insert("outputs".into(), json!([]));
        } else {
            c.remove("execution_count");
            c.remove("outputs");
        }
    }

    /// The notebook's language: its kernel's, else Python.
    pub fn language(&self) -> String {
        let m = &self.root["metadata"];
        m["kernelspec"]["language"].as_str().or_else(|| m["language_info"]["name"].as_str()).unwrap_or("python").to_lowercase()
    }

    /// The kernel it was last run with, by name (`python3`).
    pub fn kernel_name(&self) -> Option<String> {
        self.root["metadata"]["kernelspec"]["name"].as_str().filter(|s| !s.is_empty()).map(String::from)
    }

    /// The kernel's name for people (`Python 3 (ipykernel)`).
    pub fn kernel_display(&self) -> Option<String> {
        self.root["metadata"]["kernelspec"]["display_name"].as_str().filter(|s| !s.is_empty()).map(String::from)
    }

    /// An 8-character id no other cell has.
    fn new_id(&self) -> String {
        let taken: Vec<&str> = self.cells().iter().filter_map(|c| c["id"].as_str()).collect();
        // A hash of what's there and a counter: no randomness in the
        // sandbox is needed for ids that only have to differ.
        let mut seed = 0xcbf2_9ce4_8422_2325u64 ^ (self.len() as u64).wrapping_mul(0x100_0000_01b3);
        for id in &taken {
            for b in id.bytes() {
                seed = (seed ^ b as u64).wrapping_mul(0x100_0000_01b3);
            }
        }
        loop {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let id = format!("{:08x}", seed as u32);
            if !taken.contains(&id.as_str()) {
                return id;
            }
        }
    }
}

/// `v` with every object's keys in order, as Jupyter writes them
/// (`sort_keys=True`), whatever order serde_json keeps them in.
fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            Value::Object(keys.into_iter().map(|k| (k.clone(), sorted(&m[k]))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAVED: &str = include_str!("../tests/fixtures/assignment.ipynb");

    #[test]
    fn a_notebook_jupyter_saved_is_written_back_as_it_was() {
        let nb = Notebook::parse(SAVED).unwrap();
        assert_eq!(nb.to_text(), SAVED);
    }

    #[test]
    fn its_cells_are_read() {
        let nb = Notebook::parse(SAVED).unwrap();
        assert_eq!(nb.len(), 5);
        assert_eq!(nb.kind(0), Kind::Markdown);
        assert_eq!(nb.kind(1), Kind::Code);
        assert_eq!(nb.source(1), "import math\nradius = 2\nmath.pi * radius ** 2");
        assert_eq!(nb.count(1), Some(1));
        assert_eq!(nb.outputs(1).len(), 1);
        assert_eq!(nb.language(), "python");
        assert_eq!(nb.kernel_name().as_deref(), Some("python3"));
    }

    #[test]
    fn a_source_is_written_as_lines() {
        let mut nb = Notebook::parse(SAVED).unwrap();
        nb.set_source(1, "a = 1\nb = 2\n");
        assert_eq!(nb.cells()[1]["source"], json!(["a = 1\n", "b = 2\n"]));
        assert_eq!(nb.source(1), "a = 1\nb = 2\n");
        nb.set_source(1, "");
        assert_eq!(nb.cells()[1]["source"], json!([]));
    }

    #[test]
    fn streams_join_as_jupyter_keeps_them() {
        let mut nb = Notebook::parse(SAVED).unwrap();
        nb.clear_outputs(1);
        assert_eq!(nb.count(1), None);
        nb.push_output(1, json!({"output_type": "stream", "name": "stdout", "text": "one\n"}));
        nb.push_output(1, json!({"output_type": "stream", "name": "stdout", "text": "two\n"}));
        nb.push_output(1, json!({"output_type": "stream", "name": "stderr", "text": "oops\n"}));
        let outs = nb.outputs(1);
        assert_eq!(outs.len(), 2);
        assert_eq!(outs[0]["text"], json!(["one\n", "two\n"]));
        assert_eq!(outs[1]["name"], "stderr");
    }

    #[test]
    fn a_results_text_is_lines_as_nbformat_writes_it() {
        let mut nb = Notebook::parse(SAVED).unwrap();
        nb.clear_outputs(1);
        nb.push_output(1, json!({"output_type": "execute_result", "data": {"text/plain": "   a\n0  1", "image/png": "iVBOR\n", "application/json": {"a": 1}}, "metadata": {}, "execution_count": 1}));
        let data = &nb.outputs(1)[0]["data"];
        assert_eq!(data["text/plain"], json!(["   a\n", "0  1"]));
        assert_eq!(data["image/png"], json!("iVBOR\n"), "pictures stay one string");
        assert_eq!(data["application/json"], json!({"a": 1}));
    }

    #[test]
    fn new_cells_have_ids_and_their_kinds_fields() {
        let mut nb = Notebook::parse(SAVED).unwrap();
        nb.insert(2, Kind::Code);
        nb.insert(2, Kind::Markdown);
        let (md, code) = (&nb.cells()[2], &nb.cells()[3]);
        assert_eq!(md["cell_type"], "markdown");
        assert!(md.get("outputs").is_none());
        assert_eq!(code["outputs"], json!([]));
        assert!(code["execution_count"].is_null());
        let ids: Vec<&str> = nb.cells().iter().filter_map(|c| c["id"].as_str()).collect();
        assert_eq!(ids.len(), 7);
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 7, "{ids:?}");
        assert!(ids.iter().all(|i| i.len() == 8));
    }

    #[test]
    fn a_cell_changes_kind() {
        let mut nb = Notebook::parse(SAVED).unwrap();
        nb.set_kind(1, Kind::Markdown);
        assert!(nb.cells()[1].get("outputs").is_none() && nb.cells()[1].get("execution_count").is_none());
        nb.set_kind(1, Kind::Code);
        assert_eq!(nb.cells()[1]["outputs"], json!([]));
    }

    #[test]
    fn an_empty_file_is_a_new_notebook() {
        let nb = Notebook::parse("  \n").unwrap();
        assert_eq!(nb.len(), 1);
        assert_eq!(nb.kind(0), Kind::Code);
        assert!(Notebook::parse(&nb.to_text()).is_ok());
    }

    #[test]
    fn what_isnt_one_says_why() {
        assert!(Notebook::parse("{").unwrap_err().starts_with("not a notebook"));
        assert!(Notebook::parse(r#"{"nbformat": 3, "worksheets": []}"#).unwrap_err().contains("nbformat 3"));
        assert!(Notebook::parse(r#"{"nbformat": 4}"#).unwrap_err().contains("no cells"));
    }
}
