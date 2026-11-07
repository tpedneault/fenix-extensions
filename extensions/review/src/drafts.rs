//! What the Review extension asks your coding agent (`agents.ask`, with
//! `permissions.agents`) when you press `A` in a box: a new request's
//! description, from the branch's change against its base and the
//! repository's template, and the summary that goes with your review.
//! Nothing goes to the forge from here: what it writes is yours to edit
//! and send.

use fenix_extension_api::agents::{Ask, Attach, DiffOf, Show};

use crate::git_request::RequestPage;
use crate::review_page::ReviewData;
use crate::review_store::Pending;

/// The most of a review's diff that goes to the agent, in bytes.
const MAX_DIFF: usize = 96 * 1024;

/// The ask for a new request's description.
pub fn description_ask(page: &RequestPage) -> Ask {
    let template = page.templates.get(page.template).map(|t| t.text.clone()).filter(|t| !t.trim().is_empty());
    let what = if page.forge == "GitLab" { "merge request" } else { "pull request" };
    let mut prompt = format!(
        "Write the description of a {what} for the branch {branch} into {base}: what it does and why, how it was tested, anything a reviewer should look at first.",
        branch = page.branch,
        base = page.base,
    );
    if template.is_some() {
        prompt.push_str(" Fill in the repository's template, heading by heading, keeping its headings; leave out a section that asks for something the change doesn't have.");
    }
    if let Some(key) = &page.jira {
        prompt.push_str(&format!(" It's for {key}: name it in the description."));
    }
    prompt.push_str(" Reply with only the description, in Markdown, nothing before or after it, and don't change any files.");
    let base = format!("origin/{}", page.base);
    let mut ask = Ask::new(&prompt)
        .attach(Attach::diff(DiffOf::range(&base, "HEAD")))
        .attach(Attach::commits(&format!("{base}..HEAD")))
        .show(Show::Hidden);
    if let Some(template) = template {
        ask = ask.attach(Attach::text("pull request template", &template));
    }
    ask
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

/// The comments held for the review, for the agent to go by.
fn held_comments(pending: &[Pending]) -> String {
    pending.iter().map(|p| format!("{}:{}: {}\n", p.path, p.new_line.or(p.old_line).unwrap_or(0), p.body)).collect()
}

/// The ask behind `A` in the box for the review's summary: what's posted
/// with the verdict, in the reviewer's own voice.
pub fn review_ask(data: &ReviewData, pending: &[Pending]) -> Ask {
    let request = &data.request;
    let prompt = format!(
        "Write the summary a reviewer posts with their review of this {}, in Markdown, in the first person, in under 150 words: what you make of the change overall, what has to change or deserves a second look, and what's good to keep. Go by the comments the reviewer is holding, without repeating them. Reply with only the summary, nothing before or after it, and don't change any files.",
        if request.web_url.contains("/merge_requests/") { "merge request" } else { "pull request" },
    );
    let about = format!("Title: {}\nFrom {} into {}\n\n{}", request.title, request.source_branch, request.target_branch, request.description);
    let mut ask = Ask::new(&prompt).attach(Attach::text("the request", &about)).attach(Attach::text("its diff", &review_diff(data))).show(Show::Hidden);
    if !pending.is_empty() {
        ask = ask.attach(Attach::text("the comments held for the review", &held_comments(pending)));
    }
    ask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_comments_go_with_where_they_are() {
        let held = |new_line, old_line, body: &str| Pending { path: "a.rs".into(), old_path: "a.rs".into(), old_line, new_line, start_line: None, body: body.into() };
        assert_eq!(held_comments(&[held(Some(3), None, "why?"), held(None, Some(9), "gone")]), "a.rs:3: why?\na.rs:9: gone\n");
    }
}
