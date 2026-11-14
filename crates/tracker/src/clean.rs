//! Untrusted text, made safe to show and capped (`plans/tracker-seam.md`
//! §3.1). Titles, bodies, names and messages come from other people
//! through someone else's server: a tracker cleans what it sends, and the
//! agenda cleans it again, with these.

use crate::seam::{About, Comment, Found, Issue, Move, Moves, Page, People, Person, Priorities, Priority, State};
use crate::{column, level};

/// The most a `ref` is, in bytes.
pub const REF_BYTES: usize = 200;
/// The most an id (an issue's, a state's, a person's, a move's) is, in
/// bytes. Ids are opaque: kept byte for byte, or not at all.
pub const ID_BYTES: usize = 512;
/// The most a cursor or a `remember` is, in bytes; opaque as ids are.
pub const OPAQUE_BYTES: usize = 4096;
/// The most a URL is, in bytes.
pub const URL_BYTES: usize = 2048;
/// The most a title is, in characters.
pub const TITLE: usize = 500;
/// The most a body is, in bytes.
pub const BODY_BYTES: usize = 64 * 1024;
/// The most a name, an id, a label or a state is, in characters.
pub const NAME: usize = 200;
/// The most labels an issue keeps.
pub const LABELS: usize = 50;
/// The most comments an issue keeps: the most recent.
pub const COMMENTS: usize = 100;
/// The most a comment's body is, in bytes.
pub const COMMENT_BYTES: usize = 16 * 1024;
/// The most a message is, in characters.
pub const MESSAGE: usize = 300;
/// The most a time stamp is, in characters.
pub const STAMP: usize = 64;
/// The most an answer is, in bytes.
pub const ANSWER_BYTES: usize = 8 * 1024 * 1024;
/// The most moves `transitions` gives.
pub const MOVES: usize = 100;
/// The most people `people` gives.
pub const PEOPLE: usize = 500;
/// The most priorities `priorities` gives.
pub const PRIORITIES: usize = 100;

/// A character never shown: control characters (but newline and tab,
/// which the callers decide about), DEL, C1 controls, and the invisible
/// format characters that can make text read other than it is or hide
/// text in it -- the bidirectional marks, overrides and isolates
/// (LRM, RLM, ALM among them), the zero-width characters, the word
/// joiner and invisible operators, the soft hyphen, the byte-order mark,
/// the interlinear annotation marks, the fillers that draw nothing, and
/// the tag characters (U+E0000 to U+E007F), which spell hidden ASCII.
pub fn hidden(c: char) -> bool {
    matches!(c,
        '\u{0}'..='\u{8}' | '\u{b}'..='\u{1f}' | '\u{7f}'..='\u{9f}'
        | '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{115f}' | '\u{1160}' | '\u{17b4}' | '\u{17b5}' | '\u{180b}'..='\u{180f}'
        | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}'
        | '\u{3164}' | '\u{feff}' | '\u{ffa0}' | '\u{fff9}'..='\u{fffb}'
        | '\u{1d173}'..='\u{1d17a}' | '\u{e0000}'..='\u{e007f}')
}

/// `s` with line ends as `\n` and nothing hidden.
fn strip(s: &str) -> String {
    let s = s.replace("\r\n", "\n").replace(['\r', '\u{2028}', '\u{2029}'], "\n");
    s.chars().filter(|&c| !hidden(c)).collect()
}

/// At most `max` characters, `…` at the end when cut.
fn cap_chars(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let mut cut: String = s.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// At most `max` bytes, cut at a character's edge; whether it was cut.
fn cap_bytes(s: String, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_string(), true)
}

/// One line of at most `max` characters: newlines and tabs as spaces.
pub fn line(s: &str, max: usize) -> String {
    let one: String = strip(s).chars().map(|c| if c == '\n' || c == '\t' { ' ' } else { c }).collect();
    cap_chars(one.trim().to_string(), max)
}

/// A message: one line, [`MESSAGE`] characters.
pub fn message(s: &str) -> String {
    line(s, MESSAGE)
}

/// A name, a label, a state: one line, [`NAME`] characters.
pub fn name(s: &str) -> String {
    line(s, NAME)
}

/// Text of at most `max` bytes, its lines kept; whether it was cut.
pub fn text(s: &str, max: usize) -> (String, bool) {
    cap_bytes(strip(s), max)
}

/// An id or a `ref`: nothing hidden, no whitespace, at most
/// [`REF_BYTES`]; none when it's empty or longer, since a cut id is
/// someone else's.
pub fn key(s: &str) -> Option<String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.len() <= REF_BYTES && !s.chars().any(|c| hidden(c) || c.is_whitespace());
    ok.then(|| s.to_string())
}

/// An opaque value -- an id, a cursor, a `remember` -- kept byte for
/// byte when it's at most `max` bytes, and not at all when it's longer
/// or empty: never cut, trimmed or cleaned, since it goes back to the
/// tracker as it came. Spaces are fine (`workflow::In Review`). Shown,
/// it goes through [`line`] first.
pub fn opaque(s: &str, max: usize) -> Option<String> {
    (!s.is_empty() && s.len() <= max).then(|| s.to_string())
}

/// An id that may be empty (a state's, a person's): as [`opaque`], or
/// empty.
fn id(s: &str) -> String {
    opaque(s, ID_BYTES).unwrap_or_default()
}

/// Whether an answer is too big to read at all: bigger than an answer
/// may be, and some.
pub fn too_big(answer: &str) -> bool {
    answer.len() > ANSWER_BYTES + 64 * 1024
}

/// A URL to open in a browser: `http` or `https`, nothing hidden, no
/// whitespace, at most [`URL_BYTES`], and never a user or password in it.
pub fn url(s: &str) -> Option<String> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    let scheme = ["https://", "http://"].into_iter().find(|p| lower.starts_with(p))?;
    if s.len() > URL_BYTES || s.chars().any(|c| hidden(c) || c.is_whitespace()) {
        return None;
    }
    let rest = &s[scheme.len()..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    if host.is_empty() {
        return None;
    }
    Some(format!("{}{host}{path}", &s[..scheme.len()]))
}

/// A due date, `YYYY-MM-DD`; none otherwise.
pub fn date(s: &str) -> Option<String> {
    let s = s.trim();
    let b = s.as_bytes();
    let ok = b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    ok.then(|| s.to_string())
}

/// A time stamp, as the tracker wrote it: one line, [`STAMP`] characters.
pub fn stamp(s: &str) -> String {
    line(s, STAMP)
}

fn one_of(value: &str, allowed: &[&str]) -> Option<String> {
    let v = value.trim().to_ascii_lowercase();
    allowed.contains(&v.as_str()).then_some(v)
}

impl State {
    /// Its names cleaned; a stage it doesn't know is `todo`, a column it
    /// doesn't know is its stage.
    pub fn cleaned(self) -> State {
        let stage = one_of(&self.stage, &[column::TODO, column::DOING, column::DONE]).unwrap_or_else(|| column::TODO.into());
        let column = one_of(&self.column, &column::ALL).unwrap_or_else(|| stage.clone());
        State { id: id(&self.id), name: name(&self.name), stage, column }
    }
}

impl Priority {
    pub fn cleaned(self) -> Priority {
        Priority { name: name(&self.name), level: self.level.as_deref().and_then(|l| one_of(l, &level::ALL)) }
    }
}

impl Person {
    pub fn cleaned(self) -> Person {
        Person { id: id(&self.id), name: name(&self.name) }
    }
}

impl Comment {
    pub fn cleaned(self) -> Comment {
        let (mut body, cut) = text(&self.body, COMMENT_BYTES);
        if cut {
            body.push('…');
        }
        Comment { id: id(&self.id), author: name(&self.author), body, created: stamp(&self.created) }
    }
}

impl Issue {
    /// Every field cleaned and capped; none when its `ref` can't be kept.
    pub fn cleaned(self) -> Option<Issue> {
        let key = key(&self.key)?;
        let (body, cut) = text(&self.body, BODY_BYTES);
        let mut comments: Vec<Comment> = self.comments.into_iter().map(Comment::cleaned).collect();
        if comments.len() > COMMENTS {
            comments.drain(..comments.len() - COMMENTS);
        }
        let id = match self.id.as_str() {
            "" => key.clone(),
            given => opaque(given, ID_BYTES)?,
        };
        Some(Issue {
            id,
            key,
            asked: self.asked.as_deref().and_then(|a| opaque(a, URL_BYTES)),
            url: self.url.as_deref().and_then(url),
            project: self.project.as_deref().map(name).filter(|p| !p.is_empty()),
            title: line(&self.title, TITLE),
            body,
            body_cut: self.body_cut || cut,
            state: self.state.cleaned(),
            kind: self.kind.as_deref().map(name).filter(|k| !k.is_empty()),
            priority: self.priority.map(Priority::cleaned).filter(|p| !p.name.is_empty()),
            assignee: self.assignee.map(Person::cleaned).filter(|p| !p.name.is_empty() || !p.id.is_empty()),
            mine: self.mine,
            labels: self.labels.iter().map(|l| name(l)).filter(|l| !l.is_empty()).take(LABELS).collect(),
            due: self.due.as_deref().and_then(date),
            updated: stamp(&self.updated),
            comments,
        })
    }
}

/// `issues` held to [`ANSWER_BYTES`] as JSON: comments dropped, oldest
/// first, then bodies cut, until they fit.
pub fn fit(issues: &mut [Issue]) {
    let size = |issue: &Issue| serde_json::to_vec(issue).map(|v| v.len() + 1).unwrap_or(0);
    let mut sizes: Vec<usize> = issues.iter().map(size).collect();
    let mut total: usize = sizes.iter().sum::<usize>() + 2;
    while total > ANSWER_BYTES {
        let Some(i) = (0..issues.len()).max_by_key(|&i| issues[i].comments.len()).filter(|&i| !issues[i].comments.is_empty()) else { break };
        let n = issues[i].comments.len().div_ceil(2);
        issues[i].comments.drain(..n);
        let now = size(&issues[i]);
        total = total - sizes[i] + now;
        sizes[i] = now;
    }
    while total > ANSWER_BYTES {
        let Some(i) = (0..issues.len()).max_by_key(|&i| issues[i].body.len()).filter(|&i| issues[i].body.len() > 1024) else { break };
        let half = issues[i].body.len() / 2;
        let (body, _) = cap_bytes(std::mem::take(&mut issues[i].body), half);
        issues[i].body = body;
        issues[i].body_cut = true;
        let now = size(&issues[i]);
        total = total - sizes[i] + now;
        sizes[i] = now;
    }
}

impl Page {
    /// Its issues cleaned; a cursor longer than [`OPAQUE_BYTES`] is an
    /// error, never cut.
    pub fn cleaned(self) -> Result<Page, crate::SeamError> {
        let mut issues: Vec<Issue> = self.issues.into_iter().filter_map(Issue::cleaned).collect();
        fit(&mut issues);
        let next = match self.next.as_deref() {
            None | Some("") => None,
            Some(n) => Some(opaque(n, OPAQUE_BYTES).ok_or_else(|| crate::SeamError::new(crate::code::FAILED, format!("a cursor of {} bytes, more than {OPAQUE_BYTES}", n.len())))?),
        };
        Ok(Page { issues, next })
    }
}

impl Found {
    pub fn cleaned(self) -> Found {
        let mut issues: Vec<Issue> = self.issues.into_iter().filter_map(Issue::cleaned).collect();
        fit(&mut issues);
        Found { issues, missing: self.missing.iter().filter_map(|m| opaque(m, URL_BYTES)).collect() }
    }
}

impl Move {
    /// None when its id, or what it would have kept, can't be kept whole.
    pub fn cleaned(self) -> Option<Move> {
        let remember = match self.remember.as_deref() {
            None | Some("") => None,
            Some(r) => Some(opaque(r, OPAQUE_BYTES)?),
        };
        Some(Move {
            id: opaque(&self.id, ID_BYTES)?,
            name: name(&self.name),
            to: self.to.cleaned(),
            label: self.label.as_deref().map(message).filter(|l| !l.is_empty()),
            local: self.local,
            remember,
            note: self.note.as_deref().map(message).filter(|n| !n.is_empty()),
            said: self.said.as_deref().map(message).filter(|s| !s.is_empty()),
        })
    }
}

impl Moves {
    pub fn cleaned(self) -> Moves {
        Moves {
            transitions: self.transitions.into_iter().filter_map(Move::cleaned).take(MOVES).collect(),
            ask: self.ask.map(|a| crate::seam::Ask { title: message(&a.title), note: a.note.as_deref().map(message) }),
            blocked: self.blocked.as_deref().map(message),
            why: self.why.as_deref().map(message),
        }
    }
}

impl About {
    pub fn cleaned(self) -> About {
        let mut seams: Vec<u32> = self.seams.into_iter().filter(|s| *s > 0).collect();
        if seams.is_empty() {
            seams.push(self.seam);
        }
        About {
            seam: self.seam,
            seams,
            minor: self.minor,
            id: Some(self.id).filter(|i| crate::extension_id(i)).unwrap_or_default(),
            name: Some(name(&self.name)).filter(|n| !n.is_empty()).unwrap_or_else(|| "tracker".into()),
            ready: self.ready,
            not_ready: self.not_ready.as_deref().map(message),
            can: self.can.iter().map(|c| name(c)).collect(),
            // Never more often than every five minutes, never less than daily.
            sync_minutes: if self.sync_minutes == 0 { 0 } else { self.sync_minutes.clamp(5, 24 * 60) },
            project_meta: self.project_meta.as_deref().and_then(key),
            browse: self.browse.as_deref().and_then(url).filter(|b| b.contains("%7Bref%7D") || b.contains("{ref}")),
            ref_hint: self.ref_hint.as_deref().map(name),
            blocked: self.blocked,
        }
    }
}

impl People {
    pub fn cleaned(self) -> People {
        People { people: self.people.into_iter().map(Person::cleaned).filter(|p| !p.id.is_empty()).take(PEOPLE).collect(), none: self.none.as_deref().map(message) }
    }
}

impl Priorities {
    pub fn cleaned(self) -> Priorities {
        Priorities { priorities: self.priorities.into_iter().map(Priority::cleaned).filter(|p| !p.name.is_empty()).take(PRIORITIES).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_characters_go_and_lines_stay_lines() {
        assert_eq!(line("Fix\u{1b}[31m the\nthing\t\u{202e}now", 100), "Fix[31m the thing now");
        assert_eq!(text("a\r\nb\rc\u{0}\u{85}d", 100), ("a\nb\ncd".to_string(), false));
        assert_eq!(text("tab\tkept", 100).0, "tab\tkept");
        // Invisible marks, zero-width characters and tag characters
        // spelling hidden ASCII ("IGN") all go.
        let sneaky = "Fix\u{200b} it\u{200e}\u{200f}\u{61c}\u{2060}\u{ad}\u{3164}\u{fffa}\u{e0049}\u{e0047}\u{e004e}";
        assert_eq!(line(sneaky, 100), "Fix it");
        assert_eq!(key("A-1\u{200b}"), None);
        assert_eq!(line("  padded  ", 100), "padded");
    }

    #[test]
    fn long_text_is_cut_and_says_so() {
        assert_eq!(line(&"x".repeat(600), TITLE).chars().count(), TITLE);
        assert!(line(&"x".repeat(600), TITLE).ends_with('…'));
        let (body, cut) = text(&"é".repeat(BODY_BYTES), BODY_BYTES);
        assert!(cut && body.len() <= BODY_BYTES && body.chars().all(|c| c == 'é'));
    }

    #[test]
    fn a_url_is_a_web_address_with_no_credentials() {
        assert_eq!(url("https://jira.example.com/browse/A-1").as_deref(), Some("https://jira.example.com/browse/A-1"));
        assert_eq!(url("https://me:hunter2@jira.example.com/browse/A-1").as_deref(), Some("https://jira.example.com/browse/A-1"));
        assert_eq!(url("HTTP://x.org?q=a@b").as_deref(), Some("HTTP://x.org?q=a@b"), "an @ after the host isn't a user");
        assert_eq!(url("javascript:alert(1)"), None);
        assert_eq!(url("file:///etc/passwd"), None);
        assert_eq!(url("https://x.org/a b"), None);
        assert_eq!(url("https://"), None);
    }

    #[test]
    fn opaque_values_come_back_byte_for_byte_or_not_at_all() {
        assert_eq!(opaque("workflow::In Review", ID_BYTES).as_deref(), Some("workflow::In Review"));
        assert_eq!(opaque("a\tb ", ID_BYTES).as_deref(), Some("a\tb "), "not trimmed, not cleaned");
        assert_eq!(opaque(&"c".repeat(OPAQUE_BYTES + 1), OPAQUE_BYTES), None, "never cut");
        let cursor = "x".repeat(3000);
        let page = Page { issues: vec![], next: Some(cursor.clone()) }.cleaned().unwrap();
        assert_eq!(page.next.as_deref(), Some(cursor.as_str()));
        assert!(Page { issues: vec![], next: Some("x".repeat(OPAQUE_BYTES + 1)) }.cleaned().is_err());
        let m = Move { id: "workflow::In Review".into(), name: "Review".into(), remember: Some("r".repeat(2100)), ..Default::default() };
        let m = m.cleaned().unwrap();
        assert_eq!((m.id.as_str(), m.remember.as_ref().map(String::len)), ("workflow::In Review", Some(2100)));
        assert!(Move { id: "1".into(), remember: Some("r".repeat(OPAQUE_BYTES + 1)), ..Default::default() }.cleaned().is_none());
        let s = State { id: "In Review".into(), name: "x".into(), stage: "doing".into(), column: String::new() }.cleaned();
        assert_eq!(s.id, "In Review");
        let p = Person { id: "jo smith".into(), name: "Jo".into() }.cleaned();
        assert_eq!(p.id, "jo smith");
    }

    #[test]
    fn about_keeps_a_whole_id_and_a_sane_schedule() {
        let raw = |id: &str, minutes: u32| About { seam: 1, seams: vec![], minor: 0, id: id.into(), name: "X".into(), ready: true, not_ready: None, can: vec![], sync_minutes: minutes, project_meta: None, browse: None, ref_hint: None, blocked: true };
        let a = raw("fenix.jira", 1).cleaned();
        assert_eq!((a.id.as_str(), a.sync_minutes, a.seams.as_slice()), ("fenix.jira", 5, &[1][..]));
        assert_eq!(raw("Not An Id", 0).cleaned().id, "");
        assert_eq!(raw("fenix.jira", 0).cleaned().sync_minutes, 0);
    }

    #[test]
    fn a_ref_is_kept_whole_or_not_at_all() {
        assert_eq!(key(" A-1 ").as_deref(), Some("A-1"));
        assert_eq!(key("owner/repo#12").as_deref(), Some("owner/repo#12"));
        assert_eq!(key("A 1"), None);
        assert_eq!(key("A-1\u{202e}"), None);
        assert_eq!(key(&"A".repeat(201)), None);
        assert_eq!(key(""), None);
    }

    #[test]
    fn an_issue_is_cleaned_field_by_field() {
        let raw = Issue {
            key: "A-1".into(),
            url: Some("https://u:p@x.org/A-1".into()),
            title: "One\ntwo\u{7}".into(),
            body: "body\u{1b}".into(),
            state: State { id: "3".into(), name: "In\nProgress".into(), stage: "DOING".into(), column: "sideways".into() },
            priority: Some(Priority { name: "P1".into(), level: Some("extreme".into()) }),
            labels: vec!["a".into(); 60],
            due: Some("tomorrow".into()),
            comments: (0..120).map(|i| Comment { id: i.to_string(), author: "x".into(), body: "c".into(), created: String::new() }).collect(),
            ..Default::default()
        };
        let issue = raw.cleaned().unwrap();
        assert_eq!(issue.url.as_deref(), Some("https://x.org/A-1"));
        assert_eq!(issue.title, "One two");
        assert_eq!(issue.body, "body");
        assert_eq!((issue.state.name.as_str(), issue.state.stage.as_str(), issue.state.column.as_str()), ("In Progress", "doing", "doing"));
        assert_eq!(issue.priority.unwrap().level, None, "a level it doesn't know is none");
        assert_eq!(issue.labels.len(), LABELS);
        assert_eq!(issue.due, None);
        assert_eq!(issue.comments.len(), COMMENTS);
        assert_eq!(issue.comments[0].id, "20", "the most recent kept");
        assert!(Issue { key: "bad ref".into(), ..Default::default() }.cleaned().is_none());
        assert_eq!(Issue { key: "A-1".into(), ..Default::default() }.cleaned().unwrap().id, "A-1", "no id: its ref");
        assert_eq!(Issue { key: "A-1".into(), id: "10201".into(), ..Default::default() }.cleaned().unwrap().id, "10201");
    }

    #[test]
    fn an_answer_too_big_loses_comments_then_body() {
        let big = |i: usize| Issue {
            key: format!("A-{i}"),
            body: "b".repeat(BODY_BYTES),
            comments: (0..COMMENTS).map(|_| Comment { body: "c".repeat(COMMENT_BYTES), ..Default::default() }).collect(),
            ..Default::default()
        };
        let mut issues: Vec<Issue> = (0..20).map(big).collect();
        fit(&mut issues);
        assert!(serde_json::to_vec(&issues).unwrap().len() <= ANSWER_BYTES);
        assert!(issues.iter().all(|i| i.body.len() == BODY_BYTES), "the bodies fit once comments went");
        let mut bodies: Vec<Issue> = (0..200).map(|i| Issue { comments: vec![], ..big(i) }).collect();
        fit(&mut bodies);
        assert!(serde_json::to_vec(&bodies).unwrap().len() <= ANSWER_BYTES);
        assert!(bodies.iter().any(|i| i.body_cut));
    }

    #[test]
    fn a_due_date_is_a_date() {
        assert_eq!(date("2026-10-02").as_deref(), Some("2026-10-02"));
        assert_eq!(date("2026-1-2"), None);
        assert_eq!(date("next week"), None);
    }
}
