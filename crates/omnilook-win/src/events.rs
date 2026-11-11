//! Following Explorer's selection while a preview is open (`docs/development-plan.md` §4.3): the
//! folder view's `DShellFolderViewEvents.SelectionChanged`, for clicks as well as keys. Explorer
//! calls the sink on the thread that subscribed, so that thread must pump messages.

use windows::{
    Win32::{
        Foundation::{E_NOTIMPL, RECT},
        System::{
            Com::{
                DISPATCH_FLAGS, DISPPARAMS, EXCEPINFO, IConnectionPoint, IConnectionPointContainer,
                IDispatch, IDispatch_Impl, ITypeInfo,
            },
            Variant::VARIANT,
        },
        UI::{
            Shell::{DShellFolderViewEvents, SVGIO_BACKGROUND},
            WindowsAndMessaging::GetWindowRect,
        },
    },
    core::{GUID, Interface, PCWSTR, Result, implement},
};

use crate::explorer::View;

/// `DISPID_SELECTIONCHANGED`.
const SELECTION_CHANGED: i32 = 200;

#[implement(IDispatch)]
struct Sink {
    on_change: Box<dyn Fn()>,
}

impl IDispatch_Impl for Sink_Impl {
    fn GetTypeInfoCount(&self) -> Result<u32> {
        Ok(0)
    }

    fn GetTypeInfo(&self, _: u32, _: u32) -> Result<ITypeInfo> {
        Err(E_NOTIMPL.into())
    }

    fn GetIDsOfNames(
        &self,
        _: *const GUID,
        _: *const PCWSTR,
        _: u32,
        _: u32,
        _: *mut i32,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Invoke(
        &self,
        dispid: i32,
        _: *const GUID,
        _: u32,
        _: DISPATCH_FLAGS,
        _: *const DISPPARAMS,
        _: *mut VARIANT,
        _: *mut EXCEPINFO,
        _: *mut u32,
    ) -> Result<()> {
        if dispid == SELECTION_CHANGED {
            (self.on_change)();
        }
        Ok(())
    }
}

/// A live subscription; dropping it unsubscribes.
pub struct Subscription {
    point: IConnectionPoint,
    cookie: u32,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        unsafe {
            let _ = self.point.Unadvise(self.cookie);
        }
    }
}

impl View {
    /// Calls `on_change` whenever the selection in this view changes.
    pub fn subscribe(&self, on_change: impl Fn() + 'static) -> Result<Subscription> {
        unsafe {
            let shell_view = self.browser().QueryActiveShellView()?;
            let disp: IDispatch = shell_view.GetItemObject(SVGIO_BACKGROUND)?;
            let point = disp
                .cast::<IConnectionPointContainer>()?
                .FindConnectionPoint(&DShellFolderViewEvents::IID)?;
            let sink: IDispatch = Sink {
                on_change: Box::new(on_change),
            }
            .into();
            let cookie = point.Advise(&sink)?;
            Ok(Subscription { point, cookie })
        }
    }
}

/// The screen rectangle of a window, to center a preview on.
pub fn window_rect(hwnd: windows::Win32::Foundation::HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r) }.ok().map(|_| r)
}
