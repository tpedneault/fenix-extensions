//! The PDF, seen beside the work: opened in the PDF reader (`fenix.pdf`)
//! through its service `pdf.open`, which takes `{"path", "page", "heading"}`
//! -- a full path, so the project's folder is learned from where you are
//! -- and opens the file in a pane of its own. The reader watches the file
//! it shows and reloads it, keeping its place, when it's written again, so
//! the pack opens the PDF once and every compile after that shows in it.
//!
//! The reader may not be there: `pdf.open` is answered "nothing offers
//! pdf.open" then, and the pack says what to install, and where the PDF is.
//! It has no position to go to beyond a page, and the API has no call for
//! the system's viewer (`open_url` takes http(s) only), so those are left out.

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use api::{Event, Notification, Urgency};
use fenix_lang_kit::Host;

/// The reader's extension, which the notice says to install.
pub const READER: &str = "fenix.pdf";

/// The service it's asked through.
pub const SERVICE: &str = "pdf.open";

/// Where the PDF is, and whether it's been shown.
#[derive(Debug, Default)]
pub struct Preview {
    /// The call to `pdf.open` waiting for its answer, and the PDF it asked for.
    asked: Option<(u64, String)>,
    /// The PDF the reader has been asked to open and said it did: it reloads
    /// that file, so it isn't asked again by a compile.
    pub shown: Option<String>,
}

/// `file` in the project at `root`, as the full path the reader takes.
pub fn full_path(root: &str, file: &str) -> String {
    let root = root.replace('\\', "/");
    format!("{}/{}", root.trim_end_matches('/'), file.trim_start_matches("./"))
}

impl Preview {
    /// Asks the reader to open `pdf` (a full path), at `page` when there's
    /// one. Always: it brings the PDF forward when it's open already.
    pub fn open(&mut self, pdf: &str, page: Option<u32>) {
        let payload = fenix_lang_kit::serde_json::json!({ "path": pdf, "page": page }).to_string();
        let call = api::call_service(SERVICE, &payload);
        self.asked = Some((call, pdf.to_string()));
    }

    /// After a compile: the first time, opens the PDF; after that the reader
    /// has it, and reloads it itself.
    pub fn after_compile(&mut self, pdf: &str) {
        if self.shown.as_deref() != Some(pdf) && self.asked.as_ref().map(|(_, p)| p.as_str()) != Some(pdf) {
            self.open(pdf, None);
        }
    }

    /// The reader's answer. `true` when it was this pack's call.
    pub fn handle(&mut self, host: &mut dyn Host, event: &Event) -> bool {
        let Event::ServiceReply((id, result)) = event else { return false };
        let Some((call, pdf)) = self.asked.take() else { return false };
        if call != *id {
            self.asked = Some((call, pdf));
            return false;
        }
        match result {
            Ok(_) => self.shown = Some(pdf),
            Err(why) => {
                self.shown = None;
                host.notification(&unavailable(&pdf, why));
            }
        }
        true
    }

    /// The preview forgotten: the document changed, or its pane was closed.
    pub fn forget(&mut self) {
        self.shown = None;
        self.asked = None;
    }
}

/// What to say when `pdf.open` can't be answered: the reader isn't
/// installed (nothing offers the service), or it refused.
pub fn unavailable(pdf: &str, why: &str) -> Notification {
    let body = if why.contains("nothing offers") {
        format!("The PDF is {pdf}. To see it in Fenix, install the PDF reader, {READER}, from the Extensions page: it opens the PDF in a pane and shows it again each time it's compiled.")
    } else {
        format!("The PDF reader couldn't open {pdf}: {why}")
    };
    Notification::new("No PDF preview").body(body).urgency(Urgency::Normal).tag("typst.preview")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::Fake;

    #[test]
    fn the_reader_is_asked_for_a_full_path_and_remembered_once_it_answers() {
        let fake = Fake::new();
        fake.on_service("pdf.open", Ok(""));
        let mut host = fake.host();
        let mut p = Preview::default();
        p.after_compile("/home/ada/book/main.pdf");
        let calls = fake.services();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "pdf.open");
        assert_eq!(calls[0].payload, r#"{"page":null,"path":"/home/ada/book/main.pdf"}"#);
        let reply = fake.next_event().expect("the reader answered");
        assert!(p.handle(host.as_mut(), &reply));
        assert_eq!(p.shown.as_deref(), Some("/home/ada/book/main.pdf"));

        // The next compile: the reader reloads the file itself.
        p.after_compile("/home/ada/book/main.pdf");
        assert_eq!(fake.services().len(), 1, "not asked again");
        // Asked for, it brings it forward again, at a page.
        p.open("/home/ada/book/main.pdf", Some(3));
        assert_eq!(fake.services()[1].payload, r#"{"page":3,"path":"/home/ada/book/main.pdf"}"#);
    }

    #[test]
    fn without_the_reader_it_says_what_to_install_and_where_the_pdf_is() {
        let fake = Fake::new();
        // Nothing answers pdf.open: the fake says what Fenix does.
        fake.on_service("pdf.open", Err("nothing offers pdf.open"));
        let mut host = fake.host();
        let mut p = Preview::default();
        p.after_compile("/p/main.pdf");
        let reply = fake.next_event().unwrap();
        assert!(p.handle(host.as_mut(), &reply));
        assert_eq!(p.shown, None);
        let toasts = fake.showing();
        assert_eq!(toasts.len(), 1, "{toasts:?}");
        assert_eq!((toasts[0].summary.as_str(), toasts[0].tag.as_deref()), ("No PDF preview", Some("typst.preview")));
        assert!(toasts[0].body.contains("/p/main.pdf") && toasts[0].body.contains("fenix.pdf") && toasts[0].body.contains("Extensions"), "{}", toasts[0].body);
        // The next compile asks again: it may have been installed since.
        p.after_compile("/p/main.pdf");
        assert_eq!(fake.services().len(), 2);
    }

    #[test]
    fn a_reader_that_refuses_is_told_as_it_said() {
        let fake = Fake::new();
        fake.on_service("pdf.open", Err("fenix-pdfium isn't beside fenix.exe"));
        let mut host = fake.host();
        let mut p = Preview::default();
        p.open("/p/main.pdf", None);
        let reply = fake.next_event().unwrap();
        p.handle(host.as_mut(), &reply);
        let body = fake.showing()[0].body.clone();
        assert!(body.starts_with("The PDF reader couldn't open /p/main.pdf: fenix-pdfium"), "{body}");
        // Another call's answer isn't this one's.
        p.open("/p/main.pdf", None);
        assert!(!p.handle(host.as_mut(), &Event::ServiceReply((999, Ok(String::new())))));
        assert_eq!(full_path("C:\\ada\\book\\", "./src/a.pdf"), "C:/ada/book/src/a.pdf");
    }
}
