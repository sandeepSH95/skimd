//! Finder "Open With" support.
//!
//! macOS delivers double-clicked files as an `odoc` Apple Event, which winit
//! 0.30 does not expose. We register our own handler with the process-global
//! `NSAppleEventManager`; opened paths flow through a channel into an iced
//! subscription.
//!
//! Timing matters: `-[NSApplication finishLaunching]` installs AppKit's own
//! `odoc` handler (which, with no NSDocument classes, rejects every file
//! with an error dialog) BEFORE posting `willFinishLaunching`, and the
//! launch event is dispatched right after that notification. Last
//! registration wins, so re-registering in a `willFinishLaunching` observer
//! reclaims the handler in time for both the launch event and everything
//! later. (Re-registering at `didFinishLaunching` is too late — verified
//! empirically: the launch event fires between the two notifications.)

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use iced::Subscription;
use iced::futures::channel::mpsc;
use objc2::rc::Retained;
use objc2::{ClassType, DeclaredClass, declare_class, msg_send, msg_send_id, mutability, sel};
use objc2_foundation::{
    MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSNotification,
    NSNotificationCenter, NSObject, NSObjectProtocol, NSString,
};

const K_CORE_EVENT_CLASS: u32 = 0x6165_7674; // 'aevt'
const K_AE_OPEN_DOCUMENTS: u32 = 0x6F64_6F63; // 'odoc'
const K_AE_QUIT_APPLICATION: u32 = 0x7175_6974; // 'quit'
const KEY_DIRECT_OBJECT: u32 = 0x2D2D_2D2D; // '----'

static SENDER: OnceLock<mpsc::UnboundedSender<PathBuf>> = OnceLock::new();
static RECEIVER: Mutex<Option<mpsc::UnboundedReceiver<PathBuf>>> = Mutex::new(None);

static QUIT_SENDER: OnceLock<mpsc::UnboundedSender<()>> = OnceLock::new();
static QUIT_RECEIVER: Mutex<Option<mpsc::UnboundedReceiver<()>>> = Mutex::new(None);

declare_class!(
    struct OpenHandler;

    unsafe impl ClassType for OpenHandler {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "SkimdOpenHandler";
    }

    impl DeclaredClass for OpenHandler {
        type Ivars = ();
    }

    unsafe impl NSObjectProtocol for OpenHandler {}

    unsafe impl OpenHandler {
        #[method(handleEvent:withReplyEvent:)]
        fn handle_event(
            &self,
            event: &NSAppleEventDescriptor,
            _reply: &NSAppleEventDescriptor,
        ) {
            forward_paths(event);
        }

        // winit's NSApplicationDelegate refuses app termination, so Dock
        // Quit and 'quit' Apple Events would otherwise be dead ends; we
        // route them to the app as a message instead.
        #[method(handleQuitEvent:withReplyEvent:)]
        fn handle_quit_event(
            &self,
            _event: &NSAppleEventDescriptor,
            _reply: &NSAppleEventDescriptor,
        ) {
            if let Some(sender) = QUIT_SENDER.get() {
                let _ = sender.unbounded_send(());
            }
        }

        #[method(applicationWillFinishLaunching:)]
        fn application_will_finish_launching(&self, _notification: &NSNotification) {
            register_event_handlers(self);
        }
    }
);

fn register_event_handlers(handler: &OpenHandler) {
    unsafe {
        let manager = NSAppleEventManager::sharedAppleEventManager();
        let _: () = msg_send![
            &*manager,
            setEventHandler: handler as &OpenHandler,
            andSelector: sel!(handleEvent:withReplyEvent:),
            forEventClass: K_CORE_EVENT_CLASS,
            andEventID: K_AE_OPEN_DOCUMENTS,
        ];
        let _: () = msg_send![
            &*manager,
            setEventHandler: handler as &OpenHandler,
            andSelector: sel!(handleQuitEvent:withReplyEvent:),
            forEventClass: K_CORE_EVENT_CLASS,
            andEventID: K_AE_QUIT_APPLICATION,
        ];
    }
}

fn forward_paths(event: &NSAppleEventDescriptor) {
    let Some(sender) = SENDER.get() else { return };

    let list: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send_id![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
    let Some(list) = list else { return };

    for index in 1..=unsafe { list.numberOfItems() } {
        let Some(item) = (unsafe { list.descriptorAtIndex(index) }) else {
            continue;
        };
        let Some(url) = (unsafe { item.fileURLValue() }) else {
            continue;
        };
        let Some(path) = (unsafe { url.path() }) else {
            continue;
        };
        let _ = sender.unbounded_send(PathBuf::from(path.to_string()));
    }
}

/// Must run on the main thread, before the iced/winit event loop starts.
pub fn install_open_handler() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let (sender, receiver) = mpsc::unbounded();
    if SENDER.set(sender).is_err() {
        return;
    }
    *RECEIVER.lock().unwrap() = Some(receiver);
    let (quit_sender, quit_receiver) = mpsc::unbounded();
    let _ = QUIT_SENDER.set(quit_sender);
    *QUIT_RECEIVER.lock().unwrap() = Some(quit_receiver);

    let this = mtm.alloc::<OpenHandler>().set_ivars(());
    let handler: Retained<OpenHandler> = unsafe { msg_send_id![super(this), init] };

    // Take the handler slots now for events that arrive pre-launch, and
    // observe willFinishLaunching to take them back from AppKit (see the
    // module docs for the displacement dance).
    register_event_handlers(&handler);
    unsafe {
        let center = NSNotificationCenter::defaultCenter();
        let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
        center.addObserver_selector_name_object(
            &handler,
            sel!(applicationWillFinishLaunching:),
            Some(&name),
            None,
        );
    }

    // The handler must outlive the process's event handling; leak it rather
    // than rely on NSAppleEventManager or NSNotificationCenter retaining it.
    std::mem::forget(handler);
}

pub fn file_opens() -> Subscription<PathBuf> {
    Subscription::run(|| {
        RECEIVER
            .lock()
            .unwrap()
            .take()
            .expect("file_opens is subscribed exactly once")
    })
}

pub fn quit_requests() -> Subscription<()> {
    Subscription::run(|| {
        QUIT_RECEIVER
            .lock()
            .unwrap()
            .take()
            .expect("quit_requests is subscribed exactly once")
    })
}
