//! What the Review extension asks your coding agent (`agents.ask`, with
//! `permissions.agents`): a new request's title and description, from the
//! branch's change against its base and the repository's template, and a
//! summary of a request you're about to review. Nothing goes to the forge
//! from here: the drafts are yours to edit and send.

use fenix_extension_api::agents::{Ask, Attach, DiffOf, Show};
use serde_json::{json, Value};

use crate::git_request::RequestPage;
use crate::review_page::ReviewData;

/// The most of a review's diff that goes to the agent, in bytes.
const MAX_DIFF: usize = 96 * 1024;

/// What a description's answer holds.
pub fn description_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "title": { "type": "string" }, "body": { "type": "string" } },
        "required": ["title", "body"],
    })
}

/// The ask for a new request's title and description.
pub fn description_ask(page: &RequestPage) -> Ask {
    let template = page.templates.get(page.template).map(|t| t.text.clone()).filter(|t| !t.trim().is_empty());
    let what = if page.forge == "GitLab" { "merge request" } else { "pull request" };
    let mut prompt = format!(
        "Write the title and description of a {what} for the branch {branch} into {base}. The title: under 72 characters, saying what changes for the user. The description: what it does and why, how it was tested, anything a reviewer should look at first.",
        branch = page.branch,
        base = page.base,
    );
    if template.is_some() {
        prompt.push_str(" Fill in the repository's template, heading by heading, keeping its headings; leave out a section that asks for something the change doesn't have.");
    }
    if let Some(key) = &page.jira {
        prompt.push_str(&format!(" It's for {key}: name it in the description."));
    }
    prompt.push_str(" Don't change any files. Answer with one JSON block, in ```json fences: {\"title\": \"...\", \"body\": \"...\"}, the body in Markdown.");
    let base = format!("origin/{}", page.base);
    let mut ask = Ask::new(&prompt)
        .attach(Attach::diff(DiffOf::range(&base, "HEAD")))
        .attach(Attach::commits(&format!("{base}..HEAD")))
        .answer_as(description_schema())
        .show(Show::Thread);
    if let Some(template) = template {
        ask = ask.attach(Attach::text("pull request template", &template));
    }
    ask
}

/// A description's answer: its title and body, when it has them.
pub fn read_description(json: &Value) -> Option<(String, String)> {
    let title = json.get("title")?.as_str()?.trim().to_string();
    let body = json.get("body")?.as_str()?.trim().to_string();
    (!title.is_empty()).then_some((title, body))
}

/// The request's diff as text, cut to `MAX_DIFF`.
fn review_diff(data: &ReviewData) -> String {
    let mut out = String::new();
    for file in &data.files {
        let Some(diff) = &file.diff else {
            out.push_str(&format!("{} ({}): no diff\n", file.path, file.letter));
            continue;
        };
        let raw = format!("--- {}\n{}", file.path, diff.hunks.iter().map(|h| format!("{}\n{}\n", h.header, h.lines.iter().map(|l| l.raw()).collect::<Vec<_>>().join("\n"))).collect::<Vec<_>>().join(""));
        if out.len() + raw.len() > MAX_DIFF {
            out.push_str(&format!("\n[the rest of the diff is cut: {} files more]\n", data.files.len()));
            break;
        }
        out.push_str(&raw);
        out.push('\n');
    }
    out
}

/// The ask for a summary of a request under review.
pub fn summary_ask(data: &ReviewData) -> Ask {
    let request = &data.request;
    let prompt = format!(
        "Summarize this {} for its reviewer, in Markdown, in under 200 words: what it changes and why (one short paragraph), what's risky or deserves a careful look (a short list), and which files to read first, in order. Don't change any files, and don't review it line by line.",
        if request.web_url.contains("/merge_requests/") { "merge request" } else { "pull request" },
    );
    let about = format!("Title: {}\nFrom {} into {}\n\n{}", request.title, request.source_branch, request.target_branch, request.description);
    Ask::new(&prompt).attach(Attach::text("the request", &about)).attach(Attach::text("its diff", &review_diff(data))).show(Show::Hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_description_answer_needs_a_title() {
        assert_eq!(read_description(&json!({ "title": " Git: push ", "body": "Why\n" })), Some(("Git: push".into(), "Why".into())));
        assert_eq!(read_description(&json!({ "title": "", "body": "x" })), None);
        assert_eq!(read_description(&json!({ "body": "x" })), None);
    }

    #[test]
    fn the_schema_asks_for_both() {
        assert_eq!(description_schema()["required"], json!(["title", "body"]));
    }
}
