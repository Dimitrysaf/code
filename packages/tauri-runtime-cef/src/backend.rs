//! CEF-backed replacements for the `verso::{CefBuilder, CefviewController}` types
//! the runtime was written against. `CefBuilder` collects window/webview config;
//! `CefviewController` wraps the in-process CEF [`Browser`] (populated async once
//! the browser is created) and exposes the window+webview operations the runtime
//! calls. Webview ops are wired to the real CEF API; window ops + async browser
//! creation + the resource-request hook are stubbed (next grind steps).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cef::{
    Browser, CefString, ImplBrowser, ImplBrowserHost, ImplFrame, ImplWindow, Window,
};
use tauri::{LogicalPosition, LogicalSize};
use tauri_runtime::dpi::{PhysicalPosition, PhysicalSize, Position, Size};
use url::Url;

/// Error returned by controller operations. Implements `Display` for logging.
#[derive(Debug, Clone, Copy)]
pub struct CefError;

impl std::fmt::Display for CefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CEF operation failed")
    }
}

/// Callback used to complete (`Some`) or decline (`None`) an intercepted
/// web-resource request, handing the response body back to CEF. Called once.
pub type WebResourceResponseFn = Box<dyn FnOnce(Option<http::Response<Vec<u8>>>) + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WindowLevel {
    #[default]
    Normal,
    AlwaysOnTop,
    AlwaysOnBottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Clone, Debug)]
pub struct Icon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// A custom URI-scheme registration (e.g. `tauri://`). CEF wires these through a
/// scheme handler factory; for now this just records the scheme name.
#[derive(Clone, Debug)]
pub struct CustomProtocolBuilder {
    pub(crate) scheme: String,
}

impl CustomProtocolBuilder {
    pub fn new(scheme: &String) -> Self {
        Self {
            scheme: scheme.clone(),
        }
    }
}

/// Collects the desired window+webview configuration, then `build()`s a
/// [`CefviewController`] hosting a CEF browser.
#[derive(Clone, Debug, Default)]
pub struct CefBuilder {
    pub(crate) title: String,
    pub(crate) inner_size: Option<LogicalSize<f64>>,
    pub(crate) min_inner_size: Option<LogicalSize<f64>>,
    pub(crate) position: Option<LogicalPosition<f64>>,
    pub(crate) resizable: bool,
    pub(crate) decorated: bool,
    pub(crate) transparent: bool,
    pub(crate) focused: bool,
    pub(crate) fullscreen: bool,
    pub(crate) maximized: bool,
    pub(crate) visible: bool,
    pub(crate) theme: Option<Theme>,
    pub(crate) icon: Option<Icon>,
    pub(crate) window_level: WindowLevel,
    pub(crate) resources_directory: Option<PathBuf>,
    pub(crate) devtools_port: Option<u16>,
    pub(crate) user_scripts: Vec<String>,
    pub(crate) custom_protocols: Vec<CustomProtocolBuilder>,
}

impl CefBuilder {
    pub fn new() -> Self {
        Self {
            visible: true,
            focused: true,
            resizable: true,
            ..Default::default()
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }
    pub fn inner_size(mut self, size: LogicalSize<f64>) -> Self {
        self.inner_size = Some(size);
        self
    }
    pub fn min_inner_size(mut self, size: LogicalSize<f64>) -> Self {
        self.min_inner_size = Some(size);
        self
    }
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }
    pub fn position(mut self, position: LogicalPosition<f64>) -> Self {
        self.position = Some(position);
        self
    }
    pub fn decorated(mut self, decorated: bool) -> Self {
        self.decorated = decorated;
        self
    }
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
    pub fn fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }
    pub fn maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn window_level(mut self, level: WindowLevel) -> Self {
        self.window_level = level;
        self
    }
    pub fn resources_directory(mut self, dir: PathBuf) -> Self {
        self.resources_directory = Some(dir);
        self
    }
    pub fn devtools_port(mut self, port: u16) -> Self {
        self.devtools_port = Some(port);
        self
    }
    pub fn user_scripts(mut self, scripts: impl IntoIterator<Item = String>) -> Self {
        self.user_scripts = scripts.into_iter().collect();
        self
    }
    pub fn custom_protocols(
        mut self,
        protocols: impl IntoIterator<Item = CustomProtocolBuilder>,
    ) -> Self {
        self.custom_protocols = protocols.into_iter().collect();
        self
    }

    /// Creates the CEF browser + Views window for this config and returns a
    /// controller for it. `_path` (the verso executable path) is irrelevant for
    /// CEF (in-process).
    ///
    /// TODO: actually spawn the CEF browser (`browser_host_create_browser`) and
    /// populate the controller's `browser` once `OnAfterCreated` fires.
    pub fn build(self, _path: &Path, url: Url) -> CefviewController {
        let browser_slot = Arc::new(Mutex::new(None));
        let window_slot: crate::cef_app::WindowSlot = Arc::new(Mutex::new(None));
        let size = self.inner_size.unwrap_or(LogicalSize::new(1280.0, 720.0));
        let min = self
            .min_inner_size
            .unwrap_or(LogicalSize::new(0.0, 0.0));
        let config = crate::cef_app::WindowConfig {
            title: self.title.clone(),
            decorated: self.decorated,
            resizable: self.resizable,
            width: size.width as i32,
            height: size.height as i32,
            min_width: min.width as i32,
            min_height: min.height as i32,
            maximized: self.maximized,
            fullscreen: self.fullscreen,
        };
        crate::cef_app::create_browser(
            browser_slot.clone(),
            url.as_str(),
            self.user_scripts.clone(),
            config,
            window_slot.clone(),
        );
        CefviewController {
            browser: browser_slot,
            window: window_slot,
        }
    }
}

/// Handle to a live CEF browser (and its Views window). The browser is populated
/// asynchronously after creation (via the life-span handler's `on_after_created`),
/// hence the shared `Arc<Mutex<Option<..>>>`.
pub struct CefviewController {
    pub(crate) browser: Arc<Mutex<Option<Browser>>>,
    pub(crate) window: crate::cef_app::WindowSlot,
}

impl CefviewController {
    // ---- webview operations (real CEF once the browser exists) ----

    pub fn execute_script(&self, script: String) -> Result<(), CefError> {
        let guard = self.browser.lock().unwrap();
        let browser = guard.as_ref().ok_or(CefError)?;
        if let Some(frame) = browser.main_frame() {
            frame.execute_java_script(Some(&CefString::from(script.as_str())), None, 0);
            Ok(())
        } else {
            Err(CefError)
        }
    }

    pub fn get_current_url(&self) -> Result<Url, CefError> {
        let guard = self.browser.lock().unwrap();
        let browser = guard.as_ref().ok_or(CefError)?;
        let url = browser
            .main_frame()
            .map(|f| CefString::from(&f.url()).to_string())
            .unwrap_or_default();
        Url::parse(&url).map_err(|_| CefError)
    }

    pub fn navigate(&self, url: Url) -> Result<(), CefError> {
        let guard = self.browser.lock().unwrap();
        let browser = guard.as_ref().ok_or(CefError)?;
        if let Some(frame) = browser.main_frame() {
            frame.load_url(Some(&CefString::from(url.as_str())));
            Ok(())
        } else {
            Err(CefError)
        }
    }

    pub fn reload(&self) -> Result<(), CefError> {
        self.browser
            .lock()
            .unwrap()
            .as_ref()
            .ok_or(CefError)?
            .reload();
        Ok(())
    }

    // ---- window getters (stubbed pending the CEF Views window handle) ----

    pub fn get_title(&self) -> Result<String, CefError> {
        Ok(String::new())
    }
    pub fn get_theme(&self) -> Result<Theme, CefError> {
        Ok(Theme::Light)
    }
    pub fn get_scale_factor(&self) -> Result<f64, CefError> {
        Ok(1.0)
    }
    pub fn get_inner_position(&self) -> Result<Option<PhysicalPosition<i32>>, CefError> {
        Ok(None)
    }
    pub fn get_outer_position(&self) -> Result<Option<PhysicalPosition<i32>>, CefError> {
        Ok(None)
    }
    pub fn get_inner_size(&self) -> Result<PhysicalSize<u32>, CefError> {
        Ok(PhysicalSize {
            width: 0,
            height: 0,
        })
    }
    pub fn get_outer_size(&self) -> Result<PhysicalSize<u32>, CefError> {
        Ok(PhysicalSize {
            width: 0,
            height: 0,
        })
    }
    pub fn is_visible(&self) -> Result<bool, CefError> {
        Ok(true)
    }
    pub fn is_minimized(&self) -> Result<bool, CefError> {
        Ok(self.with_window(|w| w.is_minimized() != 0).unwrap_or(false))
    }
    pub fn is_maximized(&self) -> Result<bool, CefError> {
        Ok(self.with_window(|w| w.is_maximized() != 0).unwrap_or(false))
    }
    pub fn is_fullscreen(&self) -> Result<bool, CefError> {
        Ok(self.with_window(|w| w.is_fullscreen() != 0).unwrap_or(false))
    }

    // ---- window setters (stubbed pending the CEF Views window handle) ----

    /// Reads window state synchronously (getters). Runs on the caller's thread.
    fn with_window<R>(&self, f: impl FnOnce(&Window) -> R) -> Option<R> {
        self.window.lock().unwrap().as_ref().map(f)
    }

    /// Runs a window operation on the CEF UI thread. CEF ignores window ops issued
    /// from other threads, so setters must go through here.
    fn window_op(&self, op: impl FnOnce(&Window) + Send + 'static) {
        let window = self.window.clone();
        crate::cef_app::post_on_ui(move || {
            if let Some(window) = window.lock().unwrap().as_ref() {
                op(window);
            }
        });
    }

    pub fn set_title<S: Into<String>>(&self, title: S) -> Result<(), CefError> {
        let title = title.into();
        self.window_op(move |w| {
            w.set_title(Some(&CefString::from(title.as_str())))
        });
        Ok(())
    }
    pub fn set_visible(&self, _visible: bool) -> Result<(), CefError> {
        Ok(())
    }
    pub fn set_minimized(&self, minimized: bool) -> Result<(), CefError> {
        tracing::info!(
            target: "cef_console",
            "set_minimized({minimized}) window_present={} on_ui={}",
            self.window.lock().unwrap().is_some(),
            crate::cef_app::on_ui_thread(),
        );
        self.window_op(move |w| {
            if minimized {
                w.minimize()
            } else {
                w.restore()
            }
        });
        Ok(())
    }
    pub fn set_maximized(&self, maximized: bool) -> Result<(), CefError> {
        tracing::info!(
            target: "cef_console",
            "set_maximized({maximized}) window_present={} on_ui={}",
            self.window.lock().unwrap().is_some(),
            crate::cef_app::on_ui_thread(),
        );
        self.window_op(move |w| {
            if maximized {
                w.maximize()
            } else {
                w.restore()
            }
        });
        Ok(())
    }
    pub fn set_fullscreen(&self, fullscreen: bool) -> Result<(), CefError> {
        self.window_op(move |w| w.set_fullscreen(i32::from(fullscreen)));
        Ok(())
    }
    pub fn set_window_level(&self, _level: WindowLevel) -> Result<(), CefError> {
        Ok(())
    }
    pub fn set_theme(&self, _theme: Option<Theme>) -> Result<(), CefError> {
        Ok(())
    }
    pub fn set_size(&self, _size: Size) -> Result<(), CefError> {
        Ok(())
    }
    pub fn set_position(&self, _position: Position) -> Result<(), CefError> {
        Ok(())
    }
    pub fn focus(&self) -> Result<(), CefError> {
        self.window_op(|w| w.activate());
        Ok(())
    }
    pub fn start_dragging(&self) -> Result<(), CefError> {
        Ok(())
    }

    pub fn on_navigation_starting(
        &self,
        _f: impl Fn(Url) -> bool + Send + 'static,
    ) -> Result<(), CefError> {
        Ok(())
    }

    pub fn on_close_requested(
        &self,
        _f: impl Fn() + Send + 'static,
    ) -> Result<(), CefError> {
        Ok(())
    }

    /// Stores the Tauri IPC/custom-protocol dispatch closure and registers the
    /// `ipc` scheme handler that routes `ipc://` fetches to it.
    pub fn on_web_resource_requested<F>(&self, handler: F) -> Result<(), CefError>
    where
        F: FnMut(http::Request<Vec<u8>>, WebResourceResponseFn) + Send + 'static,
    {
        crate::ipc::set_ipc_handler(Box::new(handler));
        crate::ipc::register_ipc_scheme_factory();
        Ok(())
    }

    pub fn exit(&self) -> Result<(), CefError> {
        if let Some(browser) = self.browser.lock().unwrap().as_ref() {
            if let Some(host) = browser.host() {
                host.close_browser(1);
            }
        }
        Ok(())
    }
}
