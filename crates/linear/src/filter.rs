//! A typed filter, for the page's "a typed filter" view: words and
//! `key:value` pairs read into the `IssueFilter` Linear takes as a
//! variable. Nothing a person types reaches the query's text: it all
//! becomes values in this JSON.
//!
//! ```text
//! team:ENG state:started assignee:me label:bug "login page"
//! ```
//!
//! | Key | Takes |
//! |---|---|
//! | `team` | a team's key |
//! | `state` | a state's name (`"In Review"`), or a type: `triage`, `backlog`, `unstarted`, `started`, `completed`, `canceled` |
//! | `assignee`, `creator` | `me`, `none` (assignee), or part of a name |
//! | `label` | a label's name |
//! | `priority` | `urgent`, `high`, `medium`, `low`, `none` (or 0 to 4) |
//! | `project` | part of a project's name |
//! | `is` | `open` (the default: not completed or canceled), `closed`, `all` |
//!
//! Other words are part of the title; an identifier (`ENG-12`) is that
//! issue.

use serde_json::{json, Map, Value};

use crate::model::{priority_number, STATE_TYPES};
use crate::refs::{self, Named};

/// Words, with a double-quoted run (`label:"good first issue"`, `"login
/// page"`) one word.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The `IssueFilter` for `text`, or what's wrong with it.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut filter = Map::new();
    let mut open = true;
    let mut title: Vec<String> = Vec::new();
    let mut ands: Vec<Value> = Vec::new();
    for word in words(text) {
        let Some((key, value)) = word.split_once(':').filter(|(k, v)| !k.is_empty() && !v.is_empty() && k.bytes().all(|b| b.is_ascii_alphabetic())) else {
            if let Some(Named::Identifier { team, number }) = refs::identifier(&word) {
                ands.push(json!({ "team": { "key": { "eqIgnoreCase": team } }, "number": { "eq": number } }));
            } else {
                title.push(word);
            }
            continue;
        };
        let key = key.to_ascii_lowercase();
        let lower = value.to_ascii_lowercase();
        match key.as_str() {
            "team" => {
                filter.insert("team".into(), json!({ "key": { "eqIgnoreCase": value } }));
            }
            "state" | "status" => {
                let lower = if lower == "cancelled" { "canceled".to_string() } else { lower };
                let by = if STATE_TYPES.contains(&lower.as_str()) { json!({ "type": { "eq": lower } }) } else { json!({ "name": { "eqIgnoreCase": value } }) };
                filter.insert("state".into(), by);
                // Asking for a state by name or type is asking for it, done or not.
                open = false;
            }
            "assignee" | "creator" => {
                let by = match lower.as_str() {
                    "me" => json!({ "isMe": { "eq": true } }),
                    "none" | "nobody" | "unassigned" if key == "assignee" => json!({ "null": true }),
                    _ => json!({ "name": { "containsIgnoreCase": value } }),
                };
                filter.insert(key, by);
            }
            "label" => ands.push(json!({ "labels": { "some": { "name": { "eqIgnoreCase": value } } } })),
            "priority" => {
                let n = priority_number(value).or_else(|| lower.parse::<u8>().ok().filter(|n| *n <= 4)).ok_or_else(|| format!("priority:{value} -- urgent, high, medium, low or none"))?;
                filter.insert("priority".into(), json!({ "eq": n }));
            }
            "project" => {
                filter.insert("project".into(), json!({ "name": { "containsIgnoreCase": value } }));
            }
            "is" => match lower.as_str() {
                "open" => open = true,
                "closed" | "done" => {
                    open = false;
                    filter.insert("state".into(), json!({ "type": { "in": ["completed", "canceled"] } }));
                }
                "all" | "any" => open = false,
                _ => return Err(format!("is:{value} -- open, closed or all")),
            },
            other => return Err(format!("{other}: isn't a filter -- team, state, assignee, creator, label, priority, project or is")),
        }
    }
    if open {
        filter.entry("state").or_insert_with(|| json!({ "type": { "nin": ["completed", "canceled"] } }));
    }
    if !title.is_empty() {
        filter.insert("title".into(), json!({ "containsIgnoreCase": title.join(" ") }));
    }
    match ands.len() {
        0 => {}
        1 => {
            if let Value::Object(one) = ands.remove(0) {
                for (k, v) in one {
                    filter.insert(k, v);
                }
            }
        }
        _ => {
            filter.insert("and".into(), Value::Array(ands));
        }
    }
    Ok(Value::Object(filter))
}

/// What a filter is called on the page: the words as they were typed.
pub fn describe(text: &str) -> String {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        "Filter".into()
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_and_pairs_become_a_filter() {
        let f = parse(r#"team:eng state:started assignee:me label:"good first issue" login page"#).unwrap();
        assert_eq!(
            f,
            json!({
                "team": {"key": {"eqIgnoreCase": "eng"}},
                "state": {"type": {"eq": "started"}},
                "assignee": {"isMe": {"eq": true}},
                "labels": {"some": {"name": {"eqIgnoreCase": "good first issue"}}},
                "title": {"containsIgnoreCase": "login page"}
            })
        );
    }

    #[test]
    fn open_is_the_default_and_is_changes_it() {
        assert_eq!(parse("bug").unwrap(), json!({"state": {"type": {"nin": ["completed", "canceled"]}}, "title": {"containsIgnoreCase": "bug"}}));
        assert_eq!(parse("is:all bug").unwrap(), json!({"title": {"containsIgnoreCase": "bug"}}));
        assert_eq!(parse("is:closed").unwrap(), json!({"state": {"type": {"in": ["completed", "canceled"]}}}));
        assert_eq!(parse("state:\"In Review\"").unwrap(), json!({"state": {"name": {"eqIgnoreCase": "In Review"}}}));
        assert_eq!(parse("state:cancelled").unwrap()["state"], json!({"type": {"eq": "canceled"}}));
        assert_eq!(parse("assignee:none").unwrap()["assignee"], json!({"null": true}));
        assert_eq!(parse("creator:jo").unwrap()["creator"], json!({"name": {"containsIgnoreCase": "jo"}}));
        assert_eq!(parse("priority:urgent project:Hardening").unwrap()["priority"], json!({"eq": 1}));
        assert_eq!(parse("priority:3").unwrap()["priority"], json!({"eq": 3}));
    }

    #[test]
    fn an_identifier_is_that_issue_and_several_conditions_are_anded() {
        let f = parse("ENG-12").unwrap();
        assert_eq!(f["team"], json!({"key": {"eqIgnoreCase": "ENG"}}));
        assert_eq!(f["number"], json!({"eq": 12}));
        let f = parse("label:a label:b").unwrap();
        assert_eq!(f["and"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn what_cant_be_read_says_so_and_text_stays_text() {
        assert!(parse("colour:red").unwrap_err().contains("colour"));
        assert!(parse("priority:huge").is_err());
        assert!(parse("is:maybe").is_err());
        // Quotes and braces are a title's text, in a variable: no way into a query.
        let f = parse(r#"} { "evil" \ ""#).unwrap();
        assert!(f["title"]["containsIgnoreCase"].as_str().unwrap().contains('}'));
        assert_eq!(describe("  team:ENG   bug "), "team:ENG bug");
        assert_eq!(describe(""), "Filter");
    }
}
