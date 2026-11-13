//! What the reader shares with the rest: what it's told of its step, the
//! keys and places it builds from pytest's, and the tests it finds that no
//! scan did, placed under their file, classes and template.

use fenix_lang_kit::tests::{At, Found, Group, ID_MAX};

use super::index::Shared;
use super::layout::split_key;
use super::SharedFacts;

/// What a step's reader reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// `--collect-only`: the tests there are.
    Listing,
    /// A run.
    Run,
}

/// What a reader knows of its step.
pub struct Ctx {
    pub index: Shared,
    pub purpose: Purpose,
    /// The folder pytest ran in, below the project, with its `/`: `""` or
    /// `services/api/`. A node id is below it, a key below the project.
    pub prefix: String,
    /// What the pack shares: a step that finds pytest missing says so.
    pub facts: Option<SharedFacts>,
}

impl Ctx {
    /// The key of node id `id`.
    pub fn key(&self, id: &str) -> String {
        format!("{}{id}", self.prefix)
    }

    /// A place pytest gave: relative to the folder it ran in, or a full
    /// path (the machine's, `machine` being the project's folder there when
    /// a result has told it), below the project where it can be.
    pub fn at(&self, file: &str, line: u32, machine: Option<&str>) -> At {
        let file = file.replace('\\', "/");
        let below = machine.and_then(|m| file.strip_prefix(m)).and_then(|rest| rest.strip_prefix('/'));
        let is_full = file.starts_with('/') || file.as_bytes().get(1) == Some(&b':');
        match (below, is_full) {
            (Some(rest), _) => At::new(format!("{}{rest}", self.prefix), line, None),
            (None, true) => At::new(file, line, None),
            (None, false) => At::new(self.key(file.trim_start_matches("./")), line, None),
        }
    }
}

/// Node id `id` as its parts: the file, the classes, the test, and the
/// case's id in its brackets, when it has one. A case's id can hold `::`
/// and brackets (`test_url[http://x::y]`): the test ends at the first
/// bracket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parts<'a> {
    pub file: &'a str,
    pub classes: Vec<&'a str>,
    pub name: &'a str,
    /// Without its outer brackets: `strings [odd] id`.
    pub case: Option<&'a str>,
    /// The id up to the case: the template's.
    pub template: &'a str,
}

pub fn parts(id: &str) -> Parts<'_> {
    let (head, case) = match id.find('[') {
        Some(at) => (&id[..at], Some(id[at + 1..].strip_suffix(']').unwrap_or(&id[at + 1..]))),
        None => (id, None),
    };
    let (file, rest) = split_key(head);
    let mut names: Vec<&str> = rest.split("::").skip(1).collect();
    let name = names.pop().unwrap_or("");
    Parts { file, classes: names, name, case, template: head }
}

/// A test the scan didn't find, as pytest names it, placed: under its file
/// and classes; a case of a `parametrize` under its template (the
/// template itself found first when no scan did, or when the scan took it
/// for a plain test: a fixture's `params` come from another file's
/// `conftest.py`). `at` is the place pytest gave, below the folder it ran
/// in. Empty for what a scan has, or a listing already gave.
pub fn found(ctx: &Ctx, id: &str, at: Option<(&str, u32)>, announced: &mut std::collections::HashSet<String>) -> Vec<Found> {
    let key = ctx.key(id);
    let p = parts(id);
    let index = ctx.index.borrow();
    if !announced.insert(key.clone()) {
        return Vec::new();
    }
    let file_key = ctx.key(p.file);
    let mut out = Vec::new();
    let place = |f: Found| match at {
        Some((file, line)) => f.at(ctx.key(file), line),
        None => f,
    };
    let groups_of = |classes: &[&str]| -> Vec<Group> {
        let mut groups = vec![Group::file(file_key.clone())];
        let mut class_key = file_key.clone();
        for c in classes {
            class_key = format!("{class_key}::{c}");
            groups.push(index.class(&class_key).cloned().unwrap_or_else(|| Group::new(fenix_lang_kit::tests::Kind::Suite, *c).in_file(file_key.clone())));
        }
        groups
    };
    let with_data = |f: Found| if key.len() > ID_MAX && key.len() <= 4096 { f.data(key.clone()) } else { f };
    match p.case {
        None => {
            if index.test(&key).is_some() {
                return out;
            }
            out.push(place(Found::test(key.clone(), p.name).under(groups_of(&p.classes))));
        }
        Some(case) => {
            if index.test(&key).is_some() && !index.test(&key).is_some_and(|e| e.template) {
                return out;
            }
            let template_key = ctx.key(p.template);
            let scanned = index.test(&template_key);
            // The template: what the scan found as one needs nothing; what it
            // found as a plain test (fixture params) or never saw is found as
            // one, once.
            if !scanned.is_some_and(|e| e.template) && announced.insert(template_key.clone()) {
                let groups = scanned.map(|e| e.groups.to_vec()).unwrap_or_else(|| groups_of(&p.classes));
                let mut template = Found::test(template_key.clone(), p.name).under(groups).template();
                if let Some(e) = scanned {
                    template = template.line(e.line);
                } else if let Some((file, line)) = at {
                    template = template.at(ctx.key(file), line);
                }
                out.push(template);
            }
            let groups = scanned.map(|e| e.groups.to_vec()).unwrap_or_else(|| groups_of(&p.classes));
            let case = Found::case(key.clone(), case, template_key).under(groups);
            // A case is where its template is: the scan's `def`, not the
            // first decorator pytest says.
            let case = match scanned {
                Some(e) => case.at(file_key.clone(), e.line),
                None => place(case),
            };
            out.push(with_data(case));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_its_file_its_classes_its_test_and_its_case() {
        let p = parts("tests/test_basic.py::TestOuter::TestInner::test_deep");
        assert_eq!((p.file, p.classes.as_slice(), p.name, p.case), ("tests/test_basic.py", ["TestOuter", "TestInner"].as_slice(), "test_deep", None));
        let p = parts("tests/test_basic.py::test_cases[strings [odd] id]");
        assert_eq!((p.name, p.case, p.template), ("test_cases", Some("strings [odd] id"), "tests/test_basic.py::test_cases"));
        let p = parts("t/test_u.py::TestA::test_url[http://x::y]");
        assert_eq!((p.classes.as_slice(), p.name, p.case), (["TestA"].as_slice(), "test_url", Some("http://x::y")), "`::` in a case's id");
        let p = parts("tests/test_basic.py");
        assert_eq!((p.file, p.name, p.case), ("tests/test_basic.py", "", None));
    }

    #[test]
    fn a_place_is_below_the_folder_pytest_ran_in_or_a_full_path() {
        let ctx = Ctx { index: Default::default(), purpose: Purpose::Run, prefix: "services/api/".into(), facts: None };
        assert_eq!(ctx.at("tests/test_a.py", 9, None), At::new("services/api/tests/test_a.py", 9, None));
        assert_eq!(ctx.at("./tests\\test_a.py", 9, None), At::new("services/api/tests/test_a.py", 9, None));
        assert_eq!(ctx.at("/usr/lib/python3.13/unittest/case.py", 900, None), At::new("/usr/lib/python3.13/unittest/case.py", 900, None));
        assert_eq!(ctx.at("/home/u/proj/services/api/conftest.py", 3, Some("/home/u/proj/services/api")), At::new("services/api/conftest.py", 3, None), "inside the folder it ran in, by what a result told");
        assert_eq!(ctx.at("C:\\work\\x.py", 3, None), At::new("C:/work/x.py", 3, None));
    }
}
