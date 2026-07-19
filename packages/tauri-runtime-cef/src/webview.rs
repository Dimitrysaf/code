#![allow(unused_variables)]

use tauri_runtime::{
    Error, Result, UserEvent, WebviewDispatch, WebviewEventId,
    dpi::{PhysicalPosition, PhysicalSize, Position, Size},
    window::{WebviewEvent, WindowId},
};
use url::Url;

use std::{
    fmt::{self, Debug},
    sync::{Arc, Mutex},
};

use crate::{CefRuntime, RuntimeContext, backend::CefviewController};

/// The Tauri [`WebviewDispatch`] for [`CefRuntime`].
#[derive(Clone)]
pub struct CefWebviewDispatcher<T: UserEvent> {
    pub(crate) id: u32,
    pub(crate) context: RuntimeContext<T>,
    pub(crate) webview: Arc<Mutex<CefviewController>>,
}

impl<T: UserEvent> Debug for CefWebviewDispatcher<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CefWebviewDispatcher")
            .field("id", &self.id)
            .field("context", &self.context)
            .field("webview", &"CefviewController")
            .finish()
    }
}

impl<T: UserEvent> WebviewDispatch<T> for CefWebviewDispatcher<T> {
    type Runtime = CefRuntime<T>;

    fn run_on_main_thread<F: FnOnce() + Send + 'static>(&self, f: F) -> Result<()> {
        self.context.run_on_main_thread(f)
    }

    /// Unsupported, has no effect when called, the callback will not be called
    fn on_webview_event<F: Fn(&WebviewEvent) + Send + 'static>(&self, f: F) -> WebviewEventId {
        self.context.next_window_event_id()
    }

    /// Unsupported, has no effect when called
    fn with_webview<F: FnOnce(Box<dyn std::any::Any>) + Send + 'static>(&self, f: F) -> Result<()> {
        Ok(())
    }

    /// Unsupported, has no effect when called
    fn set_zoom(&self, scale_factor: f64) -> Result<()> {
        Ok(())
    }

    fn eval_script<S: Into<String>>(&self, script: S) -> Result<()> {
        self.webview
            .lock()
            .unwrap()
            .execute_script(script.into())
            .map_err(|_| Error::FailedToSendMessage)?;
        Ok(())
    }

    fn url(&self) -> Result<String> {
        Ok(self
            .webview
            .lock()
            .unwrap()
            .get_current_url()
            .map_err(|_| Error::FailedToSendMessage)?
            .to_string())
    }

    fn bounds(&self) -> Result<tauri_runtime::dpi::Rect> {
        Ok(tauri_runtime::dpi::Rect {
            position: self.position()?.into(),
            size: self.size()?.into(),
        })
    }

    fn position(&self) -> Result<PhysicalPosition<i32>> {
        Ok(PhysicalPosition { x: 0, y: 0 })
    }

    fn size(&self) -> Result<PhysicalSize<u32>> {
        self.webview
            .lock()
            .unwrap()
            .get_inner_size()
            .map_err(|_| Error::FailedToSendMessage)
    }

    fn navigate(&self, url: Url) -> Result<()> {
        self.webview
            .lock()
            .unwrap()
            .navigate(url)
            .map_err(|_| Error::FailedToSendMessage)?;
        Ok(())
    }

    /// Unsupported, has no effect when called
    fn print(&self) -> Result<()> {
        Ok(())
    }

    /// Unsupported; the window owns the webview — use the parent window's method
    fn close(&self) -> Result<()> {
        Ok(())
    }

    /// Unsupported; the window owns the webview
    fn set_bounds(&self, bounds: tauri_runtime::dpi::Rect) -> Result<()> {
        Ok(())
    }

    /// Unsupported; the window owns the webview
    fn set_size(&self, _size: Size) -> Result<()> {
        Ok(())
    }

    /// Unsupported; the window owns the webview
    fn set_position(&self, _position: Position) -> Result<()> {
        Ok(())
    }

    /// Unsupported; the window owns the webview
    fn set_focus(&self) -> Result<()> {
        Ok(())
    }

    /// Unsupported, has no effect when called
    fn reparent(&self, window_id: WindowId) -> Result<()> {
        Ok(())
    }

    /// Unsupported, has no effect when called
    fn set_auto_resize(&self, auto_resize: bool) -> Result<()> {
        Ok(())
    }

    /// Unsupported, has no effect when called
    fn clear_all_browsing_data(&self) -> Result<()> {
        Ok(())
    }

    /// Unsupported; the window owns the webview
    fn hide(&self) -> Result<()> {
        Ok(())
    }

    /// Unsupported; the window owns the webview
    fn show(&self) -> Result<()> {
        Ok(())
    }

    /// Unsupported, has no effect when called
    fn set_background_color(&self, color: Option<tauri_utils::config::Color>) -> Result<()> {
        Ok(())
    }

    /// Unsupported, has no effect when called
    fn set_cookie(&self, cookie: tauri_runtime::Cookie<'_>) -> Result<()> {
        Ok(())
    }

    /// Unsupported, has no effect when called
    fn delete_cookie(&self, cookie: tauri_runtime::Cookie<'_>) -> Result<()> {
        Ok(())
    }

    /// Unsupported, has no effect when called
    #[cfg(debug_assertions)]
    fn open_devtools(&self) {}

    /// Unsupported, has no effect when called
    #[cfg(debug_assertions)]
    fn close_devtools(&self) {}

    /// Unsupported, always returns false
    #[cfg(debug_assertions)]
    fn is_devtools_open(&self) -> Result<bool> {
        Ok(false)
    }

    fn reload(&self) -> Result<()> {
        self.webview
            .lock()
            .unwrap()
            .reload()
            .map_err(|_| Error::FailedToSendMessage)?;
        Ok(())
    }

    /// Unsupported, always returns an empty vector
    fn cookies_for_url(&self, url: Url) -> Result<Vec<tauri_runtime::Cookie<'static>>> {
        Ok(Vec::new())
    }

    /// Unsupported, always returns an empty vector
    fn cookies(&self) -> Result<Vec<tauri_runtime::Cookie<'static>>> {
        Ok(Vec::new())
    }
}
