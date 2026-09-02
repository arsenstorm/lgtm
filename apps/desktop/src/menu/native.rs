//! The context menu itself: AppKit's, not ours. A menu is the one piece of
//! chrome a person compares against every other app on the machine, so it is
//! worth the objc to get the vibrancy, the SF Symbols, the keyboard walking
//! and the screen-edge behaviour for free.

// objc's macros expand a `cargo-clippy` feature check that no longer exists.
#![allow(unexpected_cfgs)]

use super::{Act, Item};
use cocoa::base::{id, nil, BOOL, NO};
use cocoa::foundation::{NSInteger, NSPoint, NSRect, NSString};
use gpui::{Pixels, Point};
use objc::declare::ClassDecl;
use objc::runtime::{Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::cell::Cell;

/// A menu sized to its longest label reads as cramped next to the system's
/// own, which carry a shortcut column. This is the floor; a long label still
/// widens past it.
/// `CGFloat`.
const MIN_WIDTH: f64 = 200.;

// `PICKED` is which row AppKit sent back. The menu blocks the thread it was
// opened on, so it is written and read by that thread between two statements.
thread_local! {
    static PICKED: Cell<Option<usize>> = const { Cell::new(None) };
    static RECEIVER: id = unsafe { receiver() };
}

/// Runs the menu and returns what was picked. Blocks until it closes: AppKit
/// tracks a menu in a run loop of its own.
pub fn popup(items: &[Item], at: Point<Pixels>) -> Option<Act> {
    unsafe {
        let view = content_view()?;
        let pool: id = msg_send![class!(NSAutoreleasePool), new];
        let menu: id = msg_send![class!(NSMenu), new];
        // Every row is ours to enable; without this AppKit walks the responder
        // chain looking for a validator and greys the lot out.
        let _: () = msg_send![menu, setAutoenablesItems: NO];
        let _: () = msg_send![menu, setMinimumWidth: MIN_WIDTH];
        for (tag, item) in items.iter().enumerate() {
            let _: () = msg_send![menu, addItem: row(item, tag)];
        }
        PICKED.with(|picked| picked.set(None));
        let _: BOOL = msg_send![menu, popUpMenuPositioningItem: nil
                                     atLocation: point(view, at)
                                     inView: view];
        let _: () = msg_send![menu, release];
        let _: () = msg_send![pool, release];
        match items.get(PICKED.with(Cell::take)?) {
            Some(Item::Row { act, .. }) => Some(act.clone()),
            _ => None,
        }
    }
}

unsafe fn row(item: &Item, tag: usize) -> id {
    let Item::Row { icon, label, .. } = item else {
        return unsafe { msg_send![class!(NSMenuItem), separatorItem] };
    };
    unsafe {
        let row: id = msg_send![class!(NSMenuItem), alloc];
        let row: id = msg_send![row, initWithTitle: string(label)
                                     action: sel!(lgtmMenuPicked:)
                                     keyEquivalent: string("")];
        let _: () = msg_send![row, autorelease];
        let _: () = msg_send![row, setTarget: RECEIVER.with(|it| *it)];
        let _: () = msg_send![row, setTag: tag as NSInteger];
        let image: id = msg_send![class!(NSImage), imageWithSystemSymbolName: string(icon)
                                                   accessibilityDescription: nil];
        if image != nil {
            let _: () = msg_send![row, setImage: image];
        }
        row
    }
}

/// One object for every menu the app ever opens: the row's tag says which was
/// picked, so the target itself carries nothing.
unsafe fn receiver() -> id {
    extern "C" fn picked(_: &Object, _: Sel, item: id) {
        let tag: NSInteger = unsafe { msg_send![item, tag] };
        PICKED.with(|picked| picked.set(Some(tag as usize)));
    }
    unsafe {
        let mut class = ClassDecl::new("LgtmMenuReceiver", class!(NSObject)).expect("menu class");
        class.add_method(
            sel!(lgtmMenuPicked:),
            picked as extern "C" fn(&Object, Sel, id),
        );
        msg_send![class.register(), new]
    }
}

/// The window's own view: what the menu is positioned inside.
unsafe fn content_view() -> Option<id> {
    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        // A right-click makes the window key before this runs, but only if the
        // app was already active; the list is what is left when it was not.
        let mut window: id = msg_send![app, keyWindow];
        if window == nil {
            window = msg_send![app, mainWindow];
        }
        if window == nil {
            let windows: id = msg_send![app, windows];
            let count: NSInteger = msg_send![windows, count];
            if count == 0 {
                return None;
            }
            window = msg_send![windows, objectAtIndex: 0];
        }
        if window == nil {
            return None;
        }
        let view: id = msg_send![window, contentView];
        (view != nil).then_some(view)
    }
}

/// gpui counts down from the top of the window; an unflipped AppKit view
/// counts up from the bottom of itself.
unsafe fn point(view: id, at: Point<Pixels>) -> NSPoint {
    unsafe {
        let flipped: BOOL = msg_send![view, isFlipped];
        let (x, y) = (f64::from(f32::from(at.x)), f64::from(f32::from(at.y)));
        if flipped != NO {
            return NSPoint::new(x, y);
        }
        let bounds: NSRect = msg_send![view, bounds];
        NSPoint::new(x, bounds.size.height - y)
    }
}

/// An autoreleased `NSString`. Every string a menu holds outlives only the
/// pool `popup` opened around it.
unsafe fn string(text: &str) -> id {
    unsafe {
        let string: id = NSString::alloc(nil).init_str(text);
        let _: () = msg_send![string, autorelease];
        string
    }
}
