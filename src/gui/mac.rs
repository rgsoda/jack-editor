//! The two things macOS does differently, and the only Objective-C in jack.
//!
//! A Dock icon is not taken from the window, and a document is not handed to
//! a program on its command line. Both are answered here, and both are the
//! same shape of answer: talk to the running `NSApplication` directly, rather
//! than relying on being launched from a bundle.

use std::path::PathBuf;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, sel, AnyThread};
use objc2_app_kit::{
    NSApplication, NSApplicationDidFinishLaunchingNotification, NSImage,
    NSApplicationWillFinishLaunchingNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSData, NSNotificationCenter,
};

/// The icon as a PNG, which is what macOS reads.
const ICON_PNG: &[u8] = include_bytes!("icon.png");

/// `'aevt'`, `'odoc'` and `'----'`: the event class and id macOS sends when
/// something is opened with jack, and the keyword its file list is under.
/// Four-character codes, as Apple has spelled these since 1991.
const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

/// Files macOS has asked for and the run loop has not collected yet. A static
/// because the handler is an Objective-C object, called by the system, with
/// no way to reach the editor except through something both can see.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Something to say about a request that went nowhere, for the status line.
/// A drop that opens nothing should say why rather than look like a program
/// that ignored you.
static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn queue(files: Vec<PathBuf>) {
    OPENED.lock().unwrap_or_else(|held| held.into_inner()).extend(files);
}

fn note(what: String) {
    NOTES.lock().unwrap_or_else(|held| held.into_inner()).push(what);
}

/// The path a descriptor stands for, however it is spelled: a file URL, or
/// the text of one.
fn path_of(item: &NSAppleEventDescriptor) -> Option<PathBuf> {
    if let Some(url) = item.fileURLValue()
        && let Some(path) = url.path()
    {
        return Some(PathBuf::from(path.to_string()));
    }
    // A descriptor that will not coerce to a URL may still be able to say
    // what it is in words - `file:///...`, or a path already.
    let text = item.stringValue()?.to_string();
    match text.strip_prefix("file://") {
        Some(rest) => Some(PathBuf::from(percent_decoded(rest))),
        None => Some(PathBuf::from(text)),
    }
}

/// `%20` back into a space, and so on for anything else a URL escaped.
fn percent_decoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut bytes = text.bytes();
    let mut pending: Vec<u8> = Vec::new();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let digits: String = bytes.by_ref().take(2).map(char::from).collect();
            if let Ok(decoded) = u8::from_str_radix(&digits, 16) {
                pending.push(decoded);
                continue;
            }
        }
        if !pending.is_empty() {
            out.push_str(&String::from_utf8_lossy(&pending));
            pending.clear();
        }
        out.push(char::from(byte));
    }
    if !pending.is_empty() {
        out.push_str(&String::from_utf8_lossy(&pending));
    }
    out
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class holds
    // nothing that needs dropping.
    #[unsafe(super(NSObject))]
    #[name = "JackOpenDocuments"]
    struct OpenDocuments;

    impl OpenDocuments {
        /// The Apple Event itself: a list of files, each a descriptor that
        /// can be asked for the URL it stands for. What arrives is a request,
        /// not a command line, so nothing here can fail loudly - a file that
        /// will not say what it is, is one file not opened.
        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn handle_open_documents(
            &self,
            event: &NSAppleEventDescriptor,
            _reply: &NSAppleEventDescriptor,
        ) {
            let Some(list) = event.paramDescriptorForKeyword(DIRECT_OBJECT) else {
                note("macOS asked jack to open something, without saying what".into());
                return;
            };
            let mut opened = Vec::new();
            // Apple Event lists are one-based, and `numberOfItems` is the
            // last index rather than one past it. One file need not arrive as
            // a list of one, though - a lone descriptor counts none, and is
            // itself the answer.
            match list.numberOfItems() {
                0 => opened.extend(path_of(&list)),
                items => {
                    for index in 1..=items {
                        let path = list.descriptorAtIndex(index).as_deref().and_then(path_of);
                        opened.extend(path);
                    }
                }
            }
            match opened.is_empty() {
                true => note("macOS asked jack to open something it could not name".into()),
                false => queue(opened),
            }
        }

        /// AppKit puts its own handler for this event in place while it
        /// launches, which takes ours out of the way - and its own does
        /// nothing here, since jack is not a document-based application.
        /// So ours goes back in afterwards, which is what these are for.
        #[unsafe(method(appLaunching:))]
        fn app_launching(&self, _notification: &AnyObject) {
            register(self);
        }
    }
);

/// Put this handler in front of the open-documents event, replacing whatever
/// was there. Doing it twice is how it stays ours.
fn register(handler: &OpenDocuments) {
    let manager = NSAppleEventManager::sharedAppleEventManager();
    // SAFETY: the handler is the class defined above, which has this exact
    // selector, and the two codes are the event it is written for.
    unsafe {
        manager.setEventHandler_andSelector_forEventClass_andEventID(
            &***handler,
            sel!(handleOpenDocuments:withReplyEvent:),
            CORE_EVENT_CLASS,
            OPEN_DOCUMENTS,
        );
    }
}

/// Ask macOS to tell us when something is opened with jack: a file dropped on
/// the Dock icon, double-clicked in Finder, or handed over by `open -a jack`.
/// None of those pass a command line - macOS sends the application an event
/// instead, and a program that does not listen for it opens nothing.
pub fn watch_for_opened_files() {
    let handler: Retained<OpenDocuments> = unsafe { msg_send![OpenDocuments::alloc(), init] };
    register(&handler);
    // A file dropped on a jack that is not running is asked for while the
    // application is still launching, and AppKit installs its own handler in
    // the middle of that. Both notifications come after it has, and both put
    // ours back: the first is where an app is meant to claim this event, and
    // the second is there in case the first has already gone by.
    let centre = NSNotificationCenter::defaultCenter();
    // SAFETY: the observer is the class defined above, which has this exact
    // selector, and both names are AppKit's own.
    unsafe {
        centre.addObserver_selector_name_object(
            &***handler,
            sel!(appLaunching:),
            Some(NSApplicationWillFinishLaunchingNotification),
            None,
        );
        centre.addObserver_selector_name_object(
            &***handler,
            sel!(appLaunching:),
            Some(NSApplicationDidFinishLaunchingNotification),
            None,
        );
    }
    // The manager does not retain its handler, and neither does the
    // notification centre: it is the program's, for as long as it runs.
    std::mem::forget(handler);
}

/// What macOS has asked for since the last look, if anything.
pub fn opened_files() -> Vec<PathBuf> {
    std::mem::take(&mut OPENED.lock().unwrap_or_else(|held| held.into_inner()))
}

/// Anything to say about a request that opened nothing.
pub fn opening_notes() -> Vec<String> {
    std::mem::take(&mut NOTES.lock().unwrap_or_else(|held| held.into_inner()))
}

/// macOS takes the Dock icon from the application bundle, and a `jack --gui`
/// started from a terminal is a binary with no bundle around it - which is
/// the generic executable icon people see. The running application can be
/// handed one directly, though, and that covers both ways of starting it:
/// from `jack.app`, where the bundle's icon is this same drawing anyway, and
/// from a shell, where there is nothing else to go on.
pub fn dock_icon() {
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(ICON_PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    // SAFETY: the main thread, which the marker is the proof of, and an image
    // that was made here and is handed over whole.
    unsafe { NSApplication::sharedApplication(main).setApplicationIconImage(Some(&image)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_gives_its_path_back_with_the_escapes_undone() {
        assert_eq!(percent_decoded("/Users/soda/a%20file.md"), "/Users/soda/a file.md");
        assert_eq!(percent_decoded("/plain/path.rs"), "/plain/path.rs");
        // Several bytes of one character, which is one escape each.
        assert_eq!(percent_decoded("/z%C5%BAd%C5%BAb%C5%82o.txt"), "/źdźbło.txt");
        // A stray percent is a percent, not the start of anything.
        assert_eq!(percent_decoded("/100%/x"), "/100%/x");
    }
}
