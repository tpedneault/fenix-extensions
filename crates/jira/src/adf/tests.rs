//! ADF documents shaped as Jira Cloud's editor writes them (the node and
//! mark names and attributes of Atlassian's ADF schema), each node type
//! flattened, and plain text taken back.

use super::*;

fn doc(content: Value) -> Value {
    json!({"type": "doc", "version": 1, "content": content})
}

fn p(text: &str) -> Value {
    json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
}

#[test]
fn paragraphs_headings_and_line_breaks() {
    let d = doc(json!([
        {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Steps"}]},
        {"type": "paragraph", "content": [
            {"type": "text", "text": "Log in as "},
            {"type": "text", "text": "admin", "marks": [{"type": "strong"}, {"type": "code"}]},
            {"type": "hardBreak"},
            {"type": "text", "text": "then wait ", "marks": [{"type": "em"}, {"type": "textColor", "attrs": {"color": "#ff5630"}}]},
            {"type": "text", "text": "5", "marks": [{"type": "subsup", "attrs": {"type": "sup"}}, {"type": "underline"}, {"type": "strike"}]},
            {"type": "text", "text": " minutes.", "marks": [{"type": "backgroundColor", "attrs": {"color": "#fedec8"}}, {"type": "annotation", "attrs": {"id": "a1", "annotationType": "inlineComment"}}]}
        ]},
        {"type": "paragraph", "content": []},
        p("Seen on staging.")
    ]));
    assert_eq!(to_text(&d), "Steps\n\nLog in as admin\nthen wait 5 minutes.\n\nSeen on staging.");
}

#[test]
fn links_mentions_emoji_cards_dates_and_statuses() {
    let d = doc(json!([{"type": "paragraph", "content": [
        {"type": "mention", "attrs": {"id": "557058:f58131cb-b67d-43c7-b30d-6b58d40bd077", "text": "@Jo Bloggs", "accessLevel": ""}},
        {"type": "text", "text": " see "},
        {"type": "text", "text": "the runbook", "marks": [{"type": "link", "attrs": {"href": "https://wiki.example.com/runbook"}}]},
        {"type": "text", "text": " or "},
        {"type": "text", "text": "https://example.com", "marks": [{"type": "link", "attrs": {"href": "https://example.com"}}]},
        {"type": "text", "text": " "},
        {"type": "emoji", "attrs": {"shortName": ":thumbsup:", "id": "1f44d", "text": "👍"}},
        {"type": "text", "text": " by "},
        {"type": "date", "attrs": {"timestamp": "1763337600000"}},
        {"type": "text", "text": " "},
        {"type": "status", "attrs": {"text": "In progress", "color": "blue", "localId": "s1"}},
        {"type": "text", "text": " "},
        {"type": "inlineCard", "attrs": {"url": "https://acme.atlassian.net/browse/OPS-3"}},
        {"type": "text", "text": " "},
        {"type": "mention", "attrs": {"id": "abc", "text": "Kim"}}
    ]}]));
    assert_eq!(
        to_text(&d),
        "@Jo Bloggs see the runbook (https://wiki.example.com/runbook) or https://example.com :thumbsup: by 2025-11-17 [IN PROGRESS] https://acme.atlassian.net/browse/OPS-3 @Kim"
    );
}

#[test]
fn lists_nest_and_ordered_ones_count_from_their_start() {
    let item = |content: Value| json!({"type": "listItem", "content": content});
    let d = doc(json!([
        {"type": "bulletList", "content": [
            item(json!([p("Log in")])),
            item(json!([p("Wait"), {"type": "orderedList", "attrs": {"order": 3}, "content": [
                item(json!([p("five minutes")])),
                item(json!([p("then\nreload"), {"type": "bulletList", "content": [item(json!([p("deep")]))]}]))
            ]}])),
        ]},
        {"type": "orderedList", "content": [item(json!([p("one")])), item(json!([p("two")]))]}
    ]));
    assert_eq!(to_text(&d), "- Log in\n- Wait\n  3. five minutes\n  4. then\n     reload\n     - deep\n\n1. one\n2. two");
}

#[test]
fn code_blocks_are_kept_as_they_are_and_quotes_marked() {
    let d = doc(json!([
        {"type": "codeBlock", "attrs": {"language": "rust"}, "content": [{"type": "text", "text": "fn main() {\n    println!(\"hi\");\n}"}]},
        {"type": "codeBlock", "content": [{"type": "text", "text": "$ make\n"}]},
        {"type": "blockquote", "content": [p("Quoted"), p("twice")]},
        {"type": "rule"},
        p("after")
    ]));
    assert_eq!(to_text(&d), "```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n```\n$ make\n```\n\n> Quoted\n>\n> twice\n\n---\n\nafter");
}

#[test]
fn tables_are_a_row_a_line() {
    let cell = |kind: &str, text: &str| json!({"type": kind, "attrs": {"colspan": 1, "rowspan": 1}, "content": [p(text)]});
    let d = doc(json!([{"type": "table", "attrs": {"isNumberColumnEnabled": false, "layout": "default", "localId": "t1"}, "content": [
        {"type": "tableRow", "content": [cell("tableHeader", "Env"), cell("tableHeader", "State")]},
        {"type": "tableRow", "content": [cell("tableCell", "staging"), {"type": "tableCell", "attrs": {}, "content": [p("down"), p("since 9:00")]}]},
        {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph"}]}]}
    ]}]));
    assert_eq!(to_text(&d), "| Env | State |\n| staging | down since 9:00 |");
}

#[test]
fn media_is_a_placeholder() {
    let d = doc(json!([
        {"type": "mediaSingle", "attrs": {"layout": "center", "width": 66.67}, "content": [
            {"type": "media", "attrs": {"id": "7f3c1d2e-6b1a-4c55-9a3e-1f0b7b6a2a10", "type": "file", "collection": "", "width": 1200, "height": 800, "alt": "screenshot.png"}},
            {"type": "caption", "content": [{"type": "text", "text": "The error"}]}
        ]},
        {"type": "mediaGroup", "content": [
            {"type": "media", "attrs": {"id": "a", "type": "file", "collection": ""}},
            {"type": "media", "attrs": {"type": "external", "url": "https://example.com/cat.png"}}
        ]},
        {"type": "paragraph", "content": [{"type": "text", "text": "see "}, {"type": "mediaInline", "attrs": {"id": "b", "type": "file", "collection": ""}}]}
    ]));
    assert_eq!(to_text(&d), "[attachment: screenshot.png]\nThe error\n\n[attachment]\n[image: https://example.com/cat.png]\n\nsee [attachment]");
}

#[test]
fn panels_expands_tasks_and_decisions() {
    let task = |state: &str, text: &str| json!({"type": "taskItem", "attrs": {"localId": "x", "state": state}, "content": [{"type": "text", "text": text}]});
    let d = doc(json!([
        {"type": "panel", "attrs": {"panelType": "warning"}, "content": [p("Don't deploy on Fridays."), p("Really.")]},
        {"type": "panel", "attrs": {"panelType": "custom", "panelIcon": ":rainbow:", "panelColor": "#E3FCEF"}, "content": [p("Custom")]},
        {"type": "expand", "attrs": {"title": "Logs"}, "content": [{"type": "nestedExpand", "attrs": {"title": "Day 1"}, "content": [p("boot")]}]},
        {"type": "taskList", "attrs": {"localId": "l1"}, "content": [task("DONE", "Reproduce"), task("TODO", "Fix"), {"type": "taskList", "attrs": {"localId": "l2"}, "content": [task("TODO", "Write a test")]}]},
        {"type": "decisionList", "attrs": {"localId": "d1"}, "content": [{"type": "decisionItem", "attrs": {"localId": "d2", "state": "DECIDED"}, "content": [{"type": "text", "text": "Ship 2.0 first"}]}]}
    ]));
    assert_eq!(
        to_text(&d),
        "Warning: Don't deploy on Fridays.\n\nReally.\n\nCustom\n\nLogs\nDay 1\nboot\n\n[x] Reproduce\n[ ] Fix\n  [ ] Write a test\n\nDecision: Ship 2.0 first"
    );
}

#[test]
fn layouts_cards_extensions_and_unknown_nodes_keep_their_text() {
    let d = doc(json!([
        {"type": "layoutSection", "content": [
            {"type": "layoutColumn", "attrs": {"width": 50}, "content": [p("Left")]},
            {"type": "layoutColumn", "attrs": {"width": 50}, "content": [p("Right")]}
        ]},
        {"type": "blockCard", "attrs": {"url": "https://github.com/acme/app/pull/12"}},
        {"type": "embedCard", "attrs": {"url": "https://www.youtube.com/watch?v=x", "layout": "center"}},
        {"type": "bodiedExtension", "attrs": {"extensionType": "com.atlassian.confluence.macro.core", "extensionKey": "info"}, "content": [p("Inside a macro")]},
        {"type": "extension", "attrs": {"extensionType": "com.atlassian.ecosystem", "extensionKey": "roadmap"}},
        {"type": "paragraph", "content": [{"type": "placeholder", "attrs": {"text": "Type something"}}, {"type": "inlineExtension", "attrs": {"extensionKey": "jira-chart"}}]},
        {"type": "someNodeFromTomorrow", "content": [p("still read")]},
        {"type": "sparkle", "text": "inline words"},
        {"type": "paragraph", "content": [{"type": "text", "text": "a "}, {"type": "glitter", "content": [{"type": "text", "text": "wrapped"}]}, {"type": "text", "text": " word"}]}
    ]));
    assert_eq!(to_text(&d), "Left\n\nRight\n\nhttps://github.com/acme/app/pull/12\n\nhttps://www.youtube.com/watch?v=x\n\nInside a macro\n\n[roadmap]\n\n[jira-chart]\n\nstill read\n\ninline words\n\na wrapped word");
}

#[test]
fn not_adf_at_all() {
    assert_eq!(to_text(&Value::Null), "");
    assert_eq!(to_text(&json!("*wiki* markup")), "*wiki* markup");
    assert_eq!(to_text(&json!({"type": "doc", "version": 1, "content": []})), "");
    assert_eq!(to_text(&json!({"type": "doc"})), "");
}

#[test]
fn plain_text_goes_as_paragraphs_and_line_breaks_only() {
    let d = from_text("Fixed in 2.1.\nDeployed today.\n\n\n- not a list, just a line\r\n**not bold**");
    assert_eq!(
        d,
        json!({"type": "doc", "version": 1, "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Fixed in 2.1."}, {"type": "hardBreak"}, {"type": "text", "text": "Deployed today."}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "- not a list, just a line"}, {"type": "hardBreak"}, {"type": "text", "text": "**not bold**"}]}
        ]})
    );
    assert_eq!(from_text(""), json!({"type": "doc", "version": 1, "content": []}));
    assert_eq!(from_text("  \n\n"), json!({"type": "doc", "version": 1, "content": []}));
}

#[test]
fn what_it_writes_reads_back_as_it_was_written() {
    for text in ["one", "one\ntwo", "one\n\ntwo\nthree", "  indented\nkept  ", "@Jo :smile: https://x.y (not a link)"] {
        assert_eq!(to_text(&from_text(text)), text, "{text:?}");
    }
}
