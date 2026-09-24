//! The two things macOS does differently, and the only Objective-C in jack.
//!
//! A Dock icon is not taken from the window, and a document is not handed to
//! a program on its command line. Both are answered here, and both are the
//! same shape of answer: talk to the running `NSApplication` directly, rather
//! than relying on being launched from a bundle.

use std::path::PathBuf;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, AnyThread};
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::{MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSData};

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
                return;
            };
            let mut opened = Vec::new();
            // Apple Event lists are one-based, and `numberOfItems` is the
            // last index rather than one past it.
            for index in 1..=list.numberOfItems() {
                let path = list
                    .descriptorAtIndex(index)
                    .and_then(|item| item.fileURLValue())
                    .and_then(|url| url.path());
                if let Some(path) = path {
                    opened.push(PathBuf::from(path.to_string()));
                }
            }
            if !opened.is_empty() {
                OPENED.lock().unwrap_or_else(|held| held.into_inner()).extend(opened);
            }
        }
    }
);

/// Ask macOS to tell us when something is opened with jack: a file dropped on
/// the Dock icon, double-clicked in Finder, or handed over by `open -a jack`.
/// None of those pass a command line - macOS sends the application an event
/// instead, and a program that does not listen for it opens nothing.
pub fn watch_for_opened_files() {
    let handler: Retained<OpenDocuments> = unsafe { msg_send![OpenDocuments::alloc(), init] };
    let manager = NSAppleEventManager::sharedAppleEventManager();
    // SAFETY: the handler is the class defined above, which has this exact
    // selector, and the two codes are the event it is written for.
    unsafe {
        manager.setEventHandler_andSelector_forEventClass_andEventID(
            &handler,
            sel!(handleOpenDocuments:withReplyEvent:),
            CORE_EVENT_CLASS,
            OPEN_DOCUMENTS,
        );
    }
    // The manager does not retain its handler, and this one has to outlive
    // every event: it is the program's, for as long as the program runs.
    std::mem::forget(handler);
}

/// What macOS has asked for since the last look, if anything.
pub fn opened_files() -> Vec<PathBuf> {
    std::mem::take(&mut OPENED.lock().unwrap_or_else(|held| held.into_inner()))
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
