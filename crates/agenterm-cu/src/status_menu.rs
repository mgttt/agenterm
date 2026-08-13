//! Menu-bar extra for the macOS hotkey host.
//!
//! No popup, no background TCC poll. Accessibility is checked only when the
//! user opens the menu, and the first item is the only place that status lives.

#![cfg(target_os = "macos")]

use objc2::mutability::MainThreadOnly;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{declare_class, msg_send_id, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuDelegate, NSMenuItem,
    NSStatusBar, NSVariableStatusItemLength,
};
use objc2_foundation::{MainThreadMarker, NSString};

declare_class!(
    struct StatusTarget;

    unsafe impl ClassType for StatusTarget {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "AgentermCuStatusTarget";
    }

    impl DeclaredClass for StatusTarget {}

    unsafe impl StatusTarget {
        #[method(openAccessibility:)]
        fn open_accessibility(&self, _sender: Option<&AnyObject>) {
            crate::ax_guide::open_accessibility_settings();
        }

        #[method(quit:)]
        fn quit(&self, _sender: Option<&AnyObject>) {
            std::process::exit(0);
        }
    }

    unsafe impl NSObjectProtocol for StatusTarget {}

    unsafe impl NSMenuDelegate for StatusTarget {
        #[allow(non_snake_case)]
        #[method(menuWillOpen:)]
        unsafe fn menuWillOpen(&self, menu: &NSMenu) {
            refresh_ax_item(menu);
        }
    }
);

pub struct StatusMenu {
    _item: Retained<objc2_app_kit::NSStatusItem>,
    _target: Retained<StatusTarget>,
}

pub fn install(mtm: MainThreadMarker) -> Option<StatusMenu> {
    let target: Retained<StatusTarget> = unsafe { msg_send_id![mtm.alloc::<StatusTarget>(), init] };
    let menu =
        unsafe { NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), &NSString::from_str("AgentermCu")) };
    let ax = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc::<NSMenuItem>(),
            &NSString::from_str("打开辅助功能…"),
            Some(sel!(openAccessibility:)),
            &NSString::from_str(""),
        )
    };
    unsafe {
        ax.setTarget(Some(&target));
    }
    menu.addItem(&ax);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc::<NSMenuItem>(),
            &NSString::from_str("退出 AgentermCu"),
            Some(sel!(quit:)),
            &NSString::from_str("q"),
        )
    };
    unsafe {
        quit.setTarget(Some(&target));
        menu.setDelegate(Some(ProtocolObject::from_ref(&*target)));
    }

    let bar = unsafe { NSStatusBar::systemStatusBar() };
    let item = unsafe { bar.statusItemWithLength(NSVariableStatusItemLength) };
    unsafe {
        item.setMenu(Some(&menu));
        if let Some(button) = item.button(mtm) {
            if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str("rectangle.split.2x1"),
                Some(&NSString::from_str("AgentermCu")),
            ) {
                image.setTemplate(true);
                button.setImage(Some(&image));
            } else {
                button.setTitle(&NSString::from_str("Cu"));
            }
            button.setToolTip(Some(&NSString::from_str("AgentermCu")));
        }
    }
    Some(StatusMenu {
        _item: item,
        _target: target,
    })
}

fn refresh_ax_item(menu: &NSMenu) {
    let Some(item) = (unsafe { menu.itemAtIndex(0) }) else {
        return;
    };
    let on = crate::ax_guide::ax_trusted();
    let title = if on {
        "辅助功能已打开"
    } else {
        "打开辅助功能（仅 AgentermCu）…"
    };
    unsafe {
        item.setTitle(&NSString::from_str(title));
        item.setState(if on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    }
}
