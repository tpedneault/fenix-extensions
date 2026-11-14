//! Every query this crate sends, and every variable it sends with them,
//! held to Linear's GraphQL schema: a field or argument that isn't there,
//! a variable of the wrong type, a filter key Linear doesn't have, an enum
//! value it doesn't know -- each fails here, and nothing else in this
//! repository could say so without a Linear workspace.
//!
//! The schema is `tests/schema/linear-subset.graphql`: the parts of Linear's
//! public schema the queries touch, nothing more (the whole is 1.5 MB).
//! `tests/schema/README.md` says which version it is from and how to write
//! it again, from the real schema, with the one ignored test at the end of
//! this file: that run checks the queries against the *whole* schema first,
//! so a field that isn't in the subset can't get in by being left out of it.
//!
//! The test runs every call the crate makes on a recorded workspace, then
//! reads each body it sent: the query as GraphQL, its variables as JSON.
//! The fixtures' answers are held to the queries too: a field in an answer
//! that its query didn't ask for is a fixture written wrong.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;

use fenix_linear_client::linear::NewIssue;
use fenix_linear_client::recorded::Recorded;
use fenix_linear_client::seam::{Seam, Settings};
use fenix_linear_client::{filter, queries, Client, Linear, View};
use fenix_tracker::call;
use graphql_parser::query as q;
use graphql_parser::schema as s;
use serde_json::{json, Value};

// -- The schema, as far as this reads it ---------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Ty {
    Named(String),
    List(Box<Ty>),
    NonNull(Box<Ty>),
}

impl Ty {
    fn of<'a, T: graphql_parser::query::Text<'a>>(t: &graphql_parser::query::Type<'a, T>) -> Ty {
        match t {
            graphql_parser::query::Type::NamedType(n) => Ty::Named(n.as_ref().to_string()),
            graphql_parser::query::Type::ListType(i) => Ty::List(Box::new(Ty::of(i))),
            graphql_parser::query::Type::NonNullType(i) => Ty::NonNull(Box::new(Ty::of(i))),
        }
    }

    fn base(&self) -> &str {
        match self {
            Ty::Named(n) => n,
            Ty::List(i) | Ty::NonNull(i) => i.base(),
        }
    }

    fn show(&self) -> String {
        match self {
            Ty::Named(n) => n.clone(),
            Ty::List(i) => format!("[{}]", i.show()),
            Ty::NonNull(i) => format!("{}!", i.show()),
        }
    }
}

#[derive(Debug, Clone)]
struct Arg {
    name: String,
    ty: Ty,
    default: Option<String>,
}

#[derive(Debug, Clone)]
struct Field {
    name: String,
    args: Vec<Arg>,
    ty: Ty,
}

#[derive(Debug, Clone)]
enum Def {
    Object(Vec<Field>),
    Input(Vec<Arg>),
    Enum(Vec<String>),
    Scalar,
}

/// What a run of the queries touched, for writing the subset.
#[derive(Default)]
struct Touched {
    fields: BTreeSet<(String, String)>,
    inputs: BTreeSet<(String, String)>,
    types: BTreeSet<String>,
}

#[derive(Default)]
struct Schema {
    types: BTreeMap<String, Def>,
    touched: RefCell<Touched>,
}

impl Schema {
    fn parse(text: &str) -> Schema {
        let doc = s::parse_schema::<String>(text).unwrap_or_else(|e| panic!("the schema doesn't parse: {e}"));
        let mut schema = Schema::default();
        for d in doc.definitions {
            let s::Definition::TypeDefinition(t) = d else { continue };
            match t {
                s::TypeDefinition::Object(o) => {
                    schema.types.insert(o.name.clone(), Def::Object(o.fields.iter().map(field).collect()));
                }
                s::TypeDefinition::Interface(i) => {
                    schema.types.insert(i.name.clone(), Def::Object(i.fields.iter().map(field).collect()));
                }
                s::TypeDefinition::InputObject(i) => {
                    schema.types.insert(i.name.clone(), Def::Input(i.fields.iter().map(arg).collect()));
                }
                s::TypeDefinition::Enum(e) => {
                    schema.types.insert(e.name.clone(), Def::Enum(e.values.iter().map(|v| v.name.clone()).collect()));
                }
                s::TypeDefinition::Scalar(sc) => {
                    schema.types.insert(sc.name.clone(), Def::Scalar);
                }
                s::TypeDefinition::Union(_) => {}
            }
        }
        schema
    }

    fn def(&self, name: &str) -> Option<&Def> {
        match name {
            "String" | "Int" | "Float" | "Boolean" | "ID" => Some(&Def::Scalar),
            _ => self.types.get(name),
        }
    }

    fn touch_field(&self, ty: &str, field: &str) {
        self.touched.borrow_mut().fields.insert((ty.into(), field.into()));
    }

    // -- Queries ---------------------------------------------------------------

    fn select(&self, parent: &str, set: &q::SelectionSet<'_, String>, frags: &HashMap<String, &q::FragmentDefinition<'_, String>>, vars: &HashMap<String, (Ty, bool)>, at: &str) -> Result<(), String> {
        let Some(Def::Object(fields)) = self.def(parent) else { return Err(format!("{at}: {parent} isn't an object type with fields")) };
        for sel in &set.items {
            match sel {
                q::Selection::Field(f) => {
                    if f.name == "__typename" {
                        continue;
                    }
                    let here = format!("{at}.{}", f.name);
                    let Some(def) = fields.iter().find(|d| d.name == f.name) else { return Err(format!("{here}: {parent} has no field {}", f.name)) };
                    self.touch_field(parent, &f.name);
                    for (name, value) in &f.arguments {
                        let Some(a) = def.args.iter().find(|a| a.name == *name) else { return Err(format!("{here}: {} has no argument {name}", f.name)) };
                        self.literal(value, a, vars, &here)?;
                    }
                    for a in &def.args {
                        if matches!(a.ty, Ty::NonNull(_)) && a.default.is_none() && !f.arguments.iter().any(|(n, _)| *n == a.name) {
                            return Err(format!("{here}: argument {} ({}) is required", a.name, a.ty.show()));
                        }
                    }
                    let inner = def.ty.base();
                    match (self.def(inner), f.selection_set.items.is_empty()) {
                        (Some(Def::Object(_)), false) => self.select(inner, &f.selection_set, frags, vars, &here)?,
                        (Some(Def::Object(_)), true) => return Err(format!("{here}: {inner} needs a selection")),
                        (Some(_), true) => {}
                        (Some(_), false) => return Err(format!("{here}: {inner} is a scalar or enum: no selection")),
                        (None, _) => return Err(format!("{here}: the schema has no type {inner}")),
                    }
                }
                q::Selection::FragmentSpread(sp) => {
                    let Some(frag) = frags.get(&sp.fragment_name) else { return Err(format!("{at}: no fragment {}", sp.fragment_name)) };
                    let q::TypeCondition::On(on) = &frag.type_condition;
                    if on != parent {
                        return Err(format!("{at}: fragment {} is on {on}, used on {parent}", sp.fragment_name));
                    }
                    self.select(parent, &frag.selection_set, frags, vars, &format!("{at}...{}", sp.fragment_name))?;
                }
                q::Selection::InlineFragment(_) => return Err(format!("{at}: no inline fragments here")),
            }
        }
        Ok(())
    }

    /// An argument's literal, or the variable in its place.
    fn literal(&self, value: &q::Value<'_, String>, a: &Arg, vars: &HashMap<String, (Ty, bool)>, at: &str) -> Result<(), String> {
        let base = a.ty.base();
        match value {
            q::Value::Variable(v) => {
                let Some((vt, has_default)) = vars.get(v) else { return Err(format!("{at}: ${v} isn't declared")) };
                if !assignable(vt, *has_default, &a.ty, a.default.is_some()) {
                    return Err(format!("{at}: ${v} is {} but argument {} is {}", vt.show(), a.name, a.ty.show()));
                }
                Ok(())
            }
            q::Value::Enum(e) => match self.def(base) {
                Some(Def::Enum(values)) if values.contains(e) => {
                    self.touched.borrow_mut().types.insert(base.into());
                    Ok(())
                }
                _ => Err(format!("{at}: {e} isn't a value of {base}")),
            },
            q::Value::Int(_) | q::Value::Float(_) => matches!(base, "Int" | "Float").then_some(()).ok_or_else(|| format!("{at}: a number for {base}")),
            q::Value::Boolean(_) => (base == "Boolean").then_some(()).ok_or_else(|| format!("{at}: a boolean for {base}")),
            _ => Ok(()),
        }
    }

    // -- Variables --------------------------------------------------------------

    fn value(&self, ty: &Ty, v: &Value, at: &str) -> Result<(), String> {
        match (ty, v) {
            (Ty::NonNull(_), Value::Null) => Err(format!("{at}: null for {}", ty.show())),
            (Ty::NonNull(inner), v) => self.value(inner, v, at),
            (_, Value::Null) => Ok(()),
            (Ty::List(inner), Value::Array(items)) => items.iter().enumerate().try_for_each(|(i, x)| self.value(inner, x, &format!("{at}[{i}]"))),
            (Ty::List(inner), v) => self.value(inner, v, at),
            (Ty::Named(name), v) => match (self.def(name), v) {
                (Some(Def::Scalar), v) => match (name.as_str(), v) {
                    ("Int", Value::Number(n)) if n.is_i64() || n.is_u64() => Ok(()),
                    ("Float", Value::Number(_)) | ("Boolean", Value::Bool(_)) => Ok(()),
                    ("Int" | "Float" | "Boolean", v) => Err(format!("{at}: {v} for {name}")),
                    // ID, String and Linear's own scalars (DateTime, TimelessDate, ...) are text.
                    (_, Value::String(_)) => Ok(()),
                    (_, v) => Err(format!("{at}: {v} for {name}")),
                },
                (Some(Def::Enum(values)), Value::String(x)) if values.contains(x) => {
                    self.touched.borrow_mut().types.insert(name.clone());
                    Ok(())
                }
                (Some(Def::Enum(_)), v) => Err(format!("{at}: {v} isn't a value of {name}")),
                (Some(Def::Input(fields)), Value::Object(map)) => {
                    for (k, x) in map {
                        let Some(f) = fields.iter().find(|f| f.name == *k) else { return Err(format!("{at}: {name} has no field {k}")) };
                        self.touched.borrow_mut().inputs.insert((name.clone(), k.clone()));
                        self.value(&f.ty, x, &format!("{at}.{k}"))?;
                    }
                    for f in fields {
                        if matches!(f.ty, Ty::NonNull(_)) && f.default.is_none() && !map.contains_key(&f.name) {
                            return Err(format!("{at}: {name}.{} ({}) is required", f.name, f.ty.show()));
                        }
                    }
                    Ok(())
                }
                (Some(Def::Input(_)), v) => Err(format!("{at}: {v} for {name}, an input object")),
                (Some(Def::Object(_)), _) => Err(format!("{at}: {name} isn't an input type")),
                (None, _) => Err(format!("{at}: the schema has no type {name}")),
            },
        }
    }

    /// A query and the variables sent with it.
    fn check(&self, query: &str, variables: &Value) -> Result<String, String> {
        let doc = q::parse_query::<String>(query).map_err(|e| format!("doesn't parse: {e}"))?;
        let mut frags = HashMap::new();
        let mut op = None;
        for d in &doc.definitions {
            match d {
                q::Definition::Fragment(f) => {
                    frags.insert(f.name.clone(), f);
                }
                q::Definition::Operation(o) => {
                    if op.replace(o).is_some() {
                        return Err("more than one operation".into());
                    }
                }
            }
        }
        let (root, name, defs, set) = match op.ok_or("no operation")? {
            q::OperationDefinition::Query(o) => ("Query", o.name.clone(), &o.variable_definitions, &o.selection_set),
            q::OperationDefinition::Mutation(o) => ("Mutation", o.name.clone(), &o.variable_definitions, &o.selection_set),
            _ => return Err("only queries and mutations".into()),
        };
        let name = name.ok_or("an unnamed operation")?;
        let vars: HashMap<String, (Ty, bool)> = defs.iter().map(|d| (d.name.clone(), (Ty::of(&d.var_type), d.default_value.is_some()))).collect();
        self.select(root, set, &frags, &vars, &name).map_err(|e| format!("{name}: {e}"))?;
        // Every variable is used, and every one sent is declared and the right shape.
        for v in vars.keys() {
            if !query.contains(&format!("${v}")) || query.matches(&format!("${v}")).count() < 2 {
                return Err(format!("{name}: ${v} is declared and never used"));
            }
        }
        let sent = variables.as_object().ok_or("variables aren't an object")?;
        for (k, v) in sent {
            let (ty, _) = vars.get(k).ok_or_else(|| format!("{name}: variable {k} sent and not declared"))?;
            self.value(ty, v, &format!("{name}(${k})"))?;
        }
        for (k, (ty, has_default)) in &vars {
            if matches!(ty, Ty::NonNull(_)) && !has_default && !sent.contains_key(k) {
                return Err(format!("{name}: ${k} ({}) is required and wasn't sent", ty.show()));
            }
        }
        Ok(name)
    }

    /// An answer's `data`, held to what the query asks for.
    fn response(&self, query: &str, data: &Value) -> Result<(), String> {
        let doc = q::parse_query::<String>(query).map_err(|e| e.to_string())?;
        let mut frags = HashMap::new();
        let mut set = None;
        for d in &doc.definitions {
            match d {
                q::Definition::Fragment(f) => {
                    frags.insert(f.name.clone(), f);
                }
                q::Definition::Operation(q::OperationDefinition::Query(o)) => set = Some(&o.selection_set),
                q::Definition::Operation(q::OperationDefinition::Mutation(o)) => set = Some(&o.selection_set),
                _ => {}
            }
        }
        shape(set.ok_or("no operation")?, &frags, data, "data")
    }

    // -- The subset -------------------------------------------------------------

    fn subset(&self) -> String {
        let t = self.touched.borrow();
        let mut out = String::from("# The parts of Linear's GraphQL schema this extension's queries touch.\n# Written by `regenerate_the_subset` (see README.md); do not edit by hand.\n\n");
        let arg = |a: &Arg| format!("{}: {}{}", a.name, a.ty.show(), a.default.as_ref().map(|d| format!(" = {d}")).unwrap_or_default());
        let mut printed: BTreeSet<&String> = BTreeSet::new();
        for (name, def) in &self.types {
            match def {
                Def::Object(fields) => {
                    let kept: Vec<&Field> = fields.iter().filter(|f| t.fields.contains(&(name.clone(), f.name.clone()))).collect();
                    if kept.is_empty() {
                        continue;
                    }
                    out.push_str(&format!("type {name} {{\n"));
                    for f in kept {
                        let args = if f.args.is_empty() { String::new() } else { format!("({})", f.args.iter().map(&arg).collect::<Vec<_>>().join(", ")) };
                        out.push_str(&format!("  {}{args}: {}\n", f.name, f.ty.show()));
                    }
                    out.push_str("}\n\n");
                    printed.insert(name);
                }
                Def::Input(fields) => {
                    let kept: Vec<&Arg> = fields.iter().filter(|f| t.inputs.contains(&(name.clone(), f.name.clone()))).collect();
                    if kept.is_empty() {
                        continue;
                    }
                    out.push_str(&format!("input {name} {{\n"));
                    for f in kept {
                        out.push_str(&format!("  {}\n", arg(f)));
                    }
                    out.push_str("}\n\n");
                }
                Def::Enum(values) if t.types.contains(name) => out.push_str(&format!("enum {name} {{\n{}}}\n\n", values.iter().map(|v| format!("  {v}\n")).collect::<String>())),
                _ => {}
            }
        }
        // Every scalar a kept field, argument or input names, so they're all declared.
        let mut scalars: BTreeSet<String> = BTreeSet::new();
        for (name, def) in &self.types {
            let tys: Vec<&Ty> = match def {
                Def::Object(fields) => fields.iter().filter(|f| t.fields.contains(&(name.clone(), f.name.clone()))).flat_map(|f| std::iter::once(&f.ty).chain(f.args.iter().map(|a| &a.ty))).collect(),
                Def::Input(fields) => fields.iter().filter(|f| t.inputs.contains(&(name.clone(), f.name.clone()))).map(|f| &f.ty).collect(),
                _ => vec![],
            };
            scalars.extend(tys.iter().map(|t| t.base().to_string()).filter(|b| matches!(self.types.get(b), Some(Def::Scalar))));
        }
        for sc in scalars {
            out.push_str(&format!("scalar {sc}\n"));
        }
        out
    }
}

fn field(f: &s::Field<'_, String>) -> Field {
    Field { name: f.name.clone(), args: f.arguments.iter().map(arg).collect(), ty: Ty::of(&f.field_type) }
}

fn arg(a: &s::InputValue<'_, String>) -> Arg {
    Arg { name: a.name.clone(), ty: Ty::of(&a.value_type), default: a.default_value.as_ref().map(|d| d.to_string()) }
}

/// Whether a variable of type `var` can stand where `want` is asked.
fn assignable(var: &Ty, var_default: bool, want: &Ty, want_default: bool) -> bool {
    match (var, want) {
        (Ty::NonNull(v), Ty::NonNull(w)) => assignable(v, false, w, false),
        (Ty::NonNull(v), w) => assignable(v, false, w, false),
        (v, Ty::NonNull(w)) => (var_default || want_default) && assignable(v, false, w, false),
        (Ty::List(v), Ty::List(w)) => assignable(v, false, w, false),
        (Ty::Named(a), Ty::Named(b)) => a == b,
        _ => false,
    }
}

/// An answer's shape: every key is a field the query asked for.
fn shape<'a>(set: &'a q::SelectionSet<'a, String>, frags: &HashMap<String, &'a q::FragmentDefinition<'a, String>>, data: &Value, at: &str) -> Result<(), String> {
    match data {
        Value::Array(items) => return items.iter().enumerate().try_for_each(|(i, x)| shape(set, frags, x, &format!("{at}[{i}]"))),
        Value::Object(map) => {
            let mut asked: HashMap<String, &'a q::SelectionSet<'a, String>> = HashMap::new();
            collect(set, frags, &mut asked);
            for (k, v) in map {
                let Some(inner) = asked.get(k) else { return Err(format!("{at}: the answer has {k}, which the query didn't ask for")) };
                if !inner.items.is_empty() {
                    shape(inner, frags, v, &format!("{at}.{k}"))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect<'a>(set: &'a q::SelectionSet<'a, String>, frags: &HashMap<String, &'a q::FragmentDefinition<'a, String>>, into: &mut HashMap<String, &'a q::SelectionSet<'a, String>>) {
    for sel in &set.items {
        match sel {
            q::Selection::Field(f) => {
                into.insert(f.alias.clone().unwrap_or_else(|| f.name.clone()), &f.selection_set);
            }
            q::Selection::FragmentSpread(sp) => {
                if let Some(frag) = frags.get(&sp.fragment_name) {
                    collect(&frag.selection_set, frags, into);
                }
            }
            _ => {}
        }
    }
}

// -- What the crate sends ------------------------------------------------------------

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

/// Every call the crate makes, on the recorded workspace.
fn scenario() -> Rc<Recorded> {
    let r = Rc::new(Recorded::workspace(fixtures()));
    let linear = Linear::new(Client::new(r.clone(), Rc::new(|| 1_800_000_000)));
    let seam = Seam::new(linear.clone(), Settings::default());
    let go = |call: &str, request: Value| {
        let mut request = request;
        request["seam"] = json!(1);
        let _ = futures_executor::block_on(seam.serve(call, &request.to_string()));
    };
    // A write to an issue not seen yet looks its id up.
    go(call::COMMENT, json!({ "ref": "ENG-77", "body": "hello" }));
    go(call::ASSIGNED, json!({ "limit": 20 }));
    go(call::ASSIGNED, json!({ "cursor": "cursor-2", "since": "2026-11-01T00:00:00.000Z" }));
    go(call::ISSUES, json!({ "refs": ["ENG-12"] }));
    go(call::ISSUES, json!({ "refs": ["ENG-12", "OPS-7", "3f2504e0-4f89-41d3-9a0c-0305e82c3301"] }));
    go(call::TRANSITIONS, json!({ "ref": "ENG-12" }));
    go(call::TRANSITION, json!({ "ref": "ENG-12", "id": "57000000-0000-4000-8000-000000000005" }));
    go(call::PEOPLE, json!({ "ref": "ENG-12" }));
    go(call::COMMENT, json!({ "ref": "ENG-12", "body": "hello" }));
    for set in [json!({ "title": "t" }), json!({ "body": "b" }), json!({ "priority": "High" }), json!({ "due": "2026-12-01" }), json!({ "due": null }), json!({ "assignee": { "id": "u", "name": "U" } })] {
        go(call::UPDATE, json!({ "ref": "ENG-12", "set": set }));
    }
    let run = |f: &dyn Fn(&Linear) -> Result<(), fenix_tracker::SeamError>| {
        let _ = f(&linear);
    };
    run(&|l| futures_executor::block_on(l.viewer()).map(|_| ()));
    for view in [
        View::Created,
        View::Cycle { team: "ENG".into() },
        View::Backlog { team: "ENG".into() },
        View::Filter(filter::parse("team:ENG state:started assignee:me creator:me label:bug priority:high project:Auth is:open login").unwrap()),
        View::Filter(filter::parse("assignee:none state:\"In Review\" is:all ENG-12").unwrap()),
        View::Filter(filter::parse("is:closed priority:none label:a label:b").unwrap()),
        View::Filter(filter::parse("state:canceled creator:amy").unwrap()),
    ] {
        run(&|l| futures_executor::block_on(l.list(&view, 25, Some("cursor-2"))).map(|_| ()));
    }
    run(&|l| futures_executor::block_on(l.teams()).map(|_| ()));
    run(&|l| futures_executor::block_on(l.team_meta("t")).map(|_| ()));
    run(&|l| futures_executor::block_on(l.set_priority("ENG-12", 3)).map(|_| ()));
    run(&|l| futures_executor::block_on(l.assign("ENG-12", None)).map(|_| ()));
    run(&|l| futures_executor::block_on(l.create(&NewIssue { team_id: "t".into(), title: "T".into(), description: "D".into(), priority: Some(2), label_ids: vec!["l".into()], assignee_id: Some("u".into()), state_id: Some("s".into()) })).map(|_| ()));
    r
}

fn subset() -> Schema {
    Schema::parse(&std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/schema/linear-subset.graphql")).expect("tests/schema/linear-subset.graphql"))
}

/// Each query and variables sent against `schema`: the operations it names.
fn run_scenario(schema: &Schema) -> BTreeSet<String> {
    let r = scenario();
    let mut seen = BTreeSet::new();
    for body in r.sent() {
        let sent: Value = serde_json::from_str(&body).unwrap();
        match schema.check(sent["query"].as_str().unwrap(), &sent["variables"]) {
            Ok(name) => {
                seen.insert(name);
            }
            Err(e) => panic!("{e}\n  variables: {}", sent["variables"]),
        }
    }
    // The fixtures' answers are what the queries ask for.
    for (op, body) in r.replies() {
        let (_, query) = queries::ALL.iter().find(|(n, _)| *n == op).unwrap_or_else(|| panic!("a recorded answer for {op}, which isn't a query"));
        let v: Value = serde_json::from_str(&body).unwrap();
        schema.response(query, &v["data"]).unwrap_or_else(|e| panic!("{op}: {e}"));
    }
    seen
}

#[test]
fn every_query_and_its_variables_are_in_linears_schema() {
    let seen = run_scenario(&subset());
    let all: BTreeSet<String> = queries::ALL.iter().map(|(n, _)| n.to_string()).collect();
    assert_eq!(seen, all, "every query the crate has is sent by this test, so every one is held to the schema");
}

#[test]
fn the_validator_catches_what_it_is_for() {
    let schema = subset();
    let ok = |query: &str, vars: Value| schema.check(query, &vars);
    assert!(ok("query Viewer { viewer { id } }", json!({})).is_ok());
    // A typo'd field, an argument that isn't there, a variable of the wrong type, a missing required one.
    assert!(ok("query Viewer { viewer { idd } }", json!({})).unwrap_err().contains("no field idd"));
    assert!(ok("query Issue($id: String!) { issue(idx: $id) { id } }", json!({ "id": "x" })).unwrap_err().contains("no argument idx"));
    assert!(ok("query Issue($id: Int!) { issue(id: $id) { id } }", json!({ "id": 1 })).unwrap_err().contains("is Int!"));
    assert!(ok("query Issue($id: String!) { issue(id: $id) { id } }", json!({})).unwrap_err().contains("required"));
    assert!(ok("query Issue($id: String!) { issue { id } }", json!({ "id": "x" })).unwrap_err().contains("is required"));
    assert!(ok("query Issue($id: String!) { issue(id: $id) { state } }", json!({ "id": "x" })).unwrap_err().contains("needs a selection"));
    assert!(ok("query Issue($id: String!) { issue(id: $id) { title { x } } }", json!({ "id": "x" })).unwrap_err().contains("no selection"));
    // A filter key Linear doesn't have, a value of the wrong kind, an unknown enum value.
    let by = |v: Value| ok("query Issues($filter: IssueFilter) { issues(filter: $filter) { nodes { id } } }", json!({ "filter": v }));
    assert!(by(json!({ "state": { "type": { "nin": ["completed"] } } })).is_ok());
    assert!(by(json!({ "status": { "eq": "x" } })).unwrap_err().contains("no field status"));
    assert!(by(json!({ "state": { "type": { "eq": 3 } } })).unwrap_err().contains("for String"));
    assert!(by(json!({ "priority": { "eq": "high" } })).unwrap_err().contains("for Float"));
    assert!(ok("query Issues { issues(orderBy: newest) { nodes { id } } }", json!({})).unwrap_err().contains("isn't a value of PaginationOrderBy"));
}

/// Writes `tests/schema/linear-subset.graphql` from the real schema:
/// `LINEAR_SCHEMA=/path/to/schema.graphql cargo test --test schema regenerate_the_subset -- --ignored`.
/// Everything the queries and the variables touch is checked against the
/// whole first; only then is the subset written.
#[test]
#[ignore = "needs Linear's whole schema: see tests/schema/README.md"]
fn regenerate_the_subset() {
    let path = std::env::var("LINEAR_SCHEMA").expect("LINEAR_SCHEMA is the path of Linear's schema.graphql");
    let whole = Schema::parse(&std::fs::read_to_string(path).unwrap());
    run_scenario(&whole);
    let text = whole.subset();
    // The subset must hold what it was made from.
    let again = Schema::parse(&text);
    run_scenario(&again);
    std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/schema/linear-subset.graphql"), text).unwrap();
}
