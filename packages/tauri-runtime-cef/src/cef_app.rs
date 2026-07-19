//! CEF process initialization.
//!
//! CEF is multi-process: the browser (main) process calls [`initialize`] and
//! runs the message loop; renderer/GPU/utility subprocesses re-enter the binary
//! and are handled by [`execute_process`], then exit. [`init`] handles both.
//!
//! Derived from the cef-rs `cefsimple` example (BSD-licensed CEF sample port).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cef::args::Args;
use cef::*;

/// Shared slot holding the live CEF Views [`Window`], populated once the window
/// is created. Used by the drag handler and the runtime's window operations.
pub type WindowSlot = Arc<Mutex<Option<Window>>>;

/// Set once the browser has fully closed, so the event loop can exit and CEF can
/// shut down cleanly (without a live browser tripping `observers_.empty()`).
static BROWSER_CLOSED: AtomicBool = AtomicBool::new(false);

/// The live browser, kept globally so `shutdown_cef` can close it and pump the
/// message loop until it's gone before calling `shutdown()`.
static GLOBAL_BROWSER: Mutex<Option<Browser>> = Mutex::new(None);

/// The live top-level window, kept globally so `shutdown_cef` can close it
/// (cascading to the browser view) for a clean teardown.
static GLOBAL_WINDOW: Mutex<Option<Window>> = Mutex::new(None);


/// Whether the browser has finished closing (used to gate a clean shutdown).
pub fn browser_closed() -> bool {
    BROWSER_CLOSED.load(Ordering::SeqCst)
}

wrap_app! {
    pub struct CefApp;

    impl App {
        // Register `ipc` as a fetchable custom scheme so the injected invoke
        // script's `fetch("ipc://localhost/<cmd>")` is allowed. Flags:
        // STANDARD(1) | SECURE(8) | CORS_ENABLED(16) | FETCH_ENABLED(64) = 89.
        fn on_register_custom_schemes(
            &self,
            registrar: Option<&mut SchemeRegistrar>,
        ) {
            if let Some(registrar) = registrar {
                const OPTIONS: ::std::os::raw::c_int = 1 | 8 | 16 | 64;
                registrar
                    .add_custom_scheme(Some(&CefString::from("ipc")), OPTIONS);
            }
        }
    }
}

wrap_client! {
    pub struct CefClient {
        browser_slot: Arc<Mutex<Option<Browser>>>,
        init_scripts: Arc<Vec<String>>,
        window_slot: Arc<Mutex<Option<Window>>>,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(CefLifeSpanHandler::new(self.browser_slot.clone()))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(CefLoadHandler::new(self.init_scripts.clone()))
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(CefDisplayHandler::new())
        }

        fn drag_handler(&self) -> Option<DragHandler> {
            Some(CefDragHandler::new(self.window_slot.clone()))
        }
    }
}

wrap_drag_handler! {
    struct CefDragHandler {
        window_slot: Arc<Mutex<Option<Window>>>,
    }

    impl DragHandler {
        // Forward the page's `-webkit-app-region: drag` regions to the window so
        // the custom titlebar can move the frameless window.
        fn on_draggable_regions_changed(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            regions: Option<&[DraggableRegion]>,
        ) {
            if let Some(window) = self.window_slot.lock().unwrap().as_ref() {
                window.set_draggable_regions(regions);
            }
        }
    }
}

wrap_display_handler! {
    struct CefDisplayHandler {}

    impl DisplayHandler {
        // Surface the page's console messages in the app's stderr so IPC/frontend
        // errors are visible in the terminal during development.
        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            source: Option<&CefString>,
            line: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let msg = message.map(|m| m.to_string()).unwrap_or_default();
            let src = source.map(|s| s.to_string()).unwrap_or_default();
            // Emit via tracing (target `cef_console`) so it shows in the terminal
            // AND is written to the session log file by the app's logger.
            tracing::info!(target: "cef_console", "{src}:{line}: {msg}");
            0
        }
    }
}

wrap_load_handler! {
    struct CefLoadHandler {
        init_scripts: Arc<Vec<String>>,
    }

    impl LoadHandler {
        // Inject Tauri's initialization scripts (which define
        // `window.__TAURI_INTERNALS__`, `invoke`, `transformCallback`, ...) into
        // the main frame before the page's own scripts run.
        fn on_load_start(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            if let Some(frame) = frame {
                if frame.is_main() != 0 {
                    for script in self.init_scripts.iter() {
                        frame.execute_java_script(
                            Some(&CefString::from(script.as_str())),
                            None,
                            0,
                        );
                    }
                }
            }
        }
    }
}

wrap_life_span_handler! {
    struct CefLifeSpanHandler {
        browser_slot: Arc<Mutex<Option<Browser>>>,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser.cloned() {
                *GLOBAL_BROWSER.lock().unwrap() = Some(browser.clone());
                *self.browser_slot.lock().unwrap() = Some(browser);
            }
        }

        // The browser is fully destroyed here — safe to let CEF shut down.
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            *self.browser_slot.lock().unwrap() = None;
            *GLOBAL_BROWSER.lock().unwrap() = None;
            BROWSER_CLOSED.store(true, Ordering::SeqCst);
        }
    }
}

wrap_browser_view_delegate! {
    struct CefBrowserViewDelegate {}

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        // Force Alloy (chromeless, app-embedding) style. The default Chrome style
        // builds the full Chrome browser UI, whose side-panel init does a
        // localized-string lookup that CHECK-fails and aborts the process.
        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }
    }
}

/// Window properties forwarded from the Tauri window builder.
#[derive(Clone, Default)]
pub struct WindowConfig {
    pub title: String,
    pub decorated: bool,
    pub resizable: bool,
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub maximized: bool,
    pub fullscreen: bool,
}

wrap_window_delegate! {
    struct CefWindowDelegate {
        browser_view: BrowserView,
        config: WindowConfig,
        window_slot: Arc<Mutex<Option<Window>>>,
    }

    impl ViewDelegate {
        // The window's minimum size — it can't be resized below this.
        fn minimum_size(&self, _view: Option<&mut View>) -> Size {
            Size {
                width: self.config.min_width,
                height: self.config.min_height,
            }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        // Match the Alloy browser so the window is a plain chromeless app window.
        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }

        // No OS titlebar/border when the app draws its own custom titlebar
        // (Tauri `decorations: false`). CEF still provides resize borders.
        fn is_frameless(
            &self,
            _window: Option<&mut Window>,
        ) -> ::std::os::raw::c_int {
            if self.config.decorated { 0 } else { 1 }
        }

        // Without these (default 0) the frameless window can't be resized or have
        // its state changed.
        fn can_resize(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            if self.config.resizable { 1 } else { 0 }
        }
        fn can_maximize(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            1
        }
        fn can_minimize(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            1
        }

        fn on_window_created(&self, window: Option<&mut Window>) {
            if let Some(window) = window {
                let mut view = View::from(&self.browser_view);
                window.add_child_view(Some(&mut view));
                window.center_window(Some(&Size {
                    width: self.config.width.max(self.config.min_width),
                    height: self.config.height.max(self.config.min_height),
                }));
                if !self.config.title.is_empty() {
                    window.set_title(Some(&CefString::from(
                        self.config.title.as_str(),
                    )));
                }
                if self.config.fullscreen {
                    window.set_fullscreen(1);
                } else if self.config.maximized {
                    window.maximize();
                }
                window.show();
                // Keep the window so the drag handler + runtime ops can reach it.
                *self.window_slot.lock().unwrap() = Some(window.clone());
                *GLOBAL_WINDOW.lock().unwrap() = Some(window.clone());
            }
        }
    }
}

/// Creates a top-level, chromeless CEF window (via the Views framework) hosting a
/// browser that loads `url`. The [`Browser`] is delivered asynchronously into
/// `browser_slot` via the life-span handler's `on_after_created`. Must be called
/// on the CEF UI thread.
/// Injected script that maps Tauri's `data-tauri-drag-region` attribute onto
/// Chromium's `-webkit-app-region`, which is what CEF reports as draggable
/// regions for moving a frameless window.
const DRAG_REGION_SCRIPT: &str = "(function(){var css='[data-tauri-drag-region]{-webkit-app-region:drag}[data-tauri-drag-region] button,[data-tauri-drag-region] a,[data-tauri-drag-region] input,[data-tauri-drag-region] select,[data-tauri-drag-region] textarea{-webkit-app-region:no-drag}';function inject(){try{var s=document.createElement('style');s.textContent=css;(document.head||document.documentElement).appendChild(s);}catch(e){document.addEventListener('DOMContentLoaded',inject);}}inject();})();";

pub fn create_browser(
    browser_slot: Arc<Mutex<Option<Browser>>>,
    url: &str,
    init_scripts: Vec<String>,
    config: WindowConfig,
    window_slot: WindowSlot,
) {
    let mut scripts = init_scripts;
    scripts.push(DRAG_REGION_SCRIPT.to_string());

    let mut client =
        CefClient::new(browser_slot, Arc::new(scripts), window_slot.clone());
    let settings = BrowserSettings::default();
    let url = CefString::from(url);
    let mut view_delegate = CefBrowserViewDelegate::new();
    let browser_view = browser_view_create(
        Some(&mut client),
        Some(&url),
        Some(&settings),
        None,
        None,
        Some(&mut view_delegate),
    )
    .expect("browser_view_create returned None");

    let mut delegate =
        CefWindowDelegate::new(browser_view, config, window_slot);
    window_create_top_level(Some(&mut delegate));
}

/// Resolves a writable, app-specific directory for CEF's root cache. Overridable
/// via `CEF_ROOT_CACHE_PATH`; otherwise under the XDG data dir.
fn root_cache_path() -> Option<String> {
    if let Ok(p) = std::env::var("CEF_ROOT_CACHE_PATH") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| format!("{h}/.local/share"))
        })?;
    Some(format!("{base}/ModrinthAppCef/cef_cache"))
}

/// Initializes CEF for this process. Must be called once, as early as possible
/// in `main()` (before any windows/browsers are created).
///
/// If this process is a CEF helper subprocess it is run to completion here and
/// the process exits. In the browser process, CEF is initialized and control
/// returns to the caller.
pub fn init() {
    // Initialize the CEF API version (required before any other CEF call).
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let sandbox_info = std::ptr::null_mut();
    let mut app = CefApp::new();

    // Renderer/GPU/utility subprocesses execute here and then exit. They must
    // receive the same App so custom schemes (`ipc`) are registered in every
    // process — otherwise `fetch("ipc://...")` fails in the renderer.
    let ret = execute_process(Some(args.as_main_args()), Some(&mut app), sandbox_info);
    if ret >= 0 {
        std::process::exit(ret);
    }
    let mut settings = Settings {
        no_sandbox: 1,
        ..Default::default()
    };
    // Point CEF at its resource/locale files (icudtl.dat, *.pak, locales/).
    // These live in the CEF distribution dir (CEF_PATH), not next to our binary,
    // so without this CEF fails to initialize when run from target/debug.
    if let Ok(cef_dir) = std::env::var("CEF_PATH") {
        settings.resources_dir_path = CefString::from(cef_dir.as_str());
        settings.locales_dir_path =
            CefString::from(format!("{cef_dir}/locales").as_str());
    }
    // A dedicated, writable cache dir. Without a customized `root_cache_path`,
    // CEF's Chrome runtime uses a default that triggers process-singleton
    // behavior, causing the browser process to exit immediately.
    if let Some(cache_dir) = root_cache_path() {
        let _ = std::fs::create_dir_all(&cache_dir);
        // Clear any stale process-singleton lock left by a previous crash or
        // force-kill; otherwise CEF thinks another instance owns the cache and
        // `initialize` fails ("Opening in existing browser session"). Real
        // single-instance enforcement is done by tauri-plugin-single-instance.
        for name in ["SingletonLock", "SingletonCookie", "SingletonSocket"] {
            let _ = std::fs::remove_file(std::path::Path::new(&cache_dir).join(name));
        }
        settings.root_cache_path = CefString::from(cache_dir.as_str());
    }
    // Dev-only: expose the DevTools remote-debugging port if requested.
    if let Ok(port) = std::env::var("CEF_DEBUG_PORT") {
        if let Ok(port) = port.parse::<::std::os::raw::c_int>() {
            settings.remote_debugging_port = port;
        }
    }
    let initialized = initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        sandbox_info,
    );
    assert_eq!(initialized, 1, "CEF `initialize` failed");
}

wrap_task! {
    struct CefTask {
        func: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>,
    }

    impl Task {
        fn execute(&self) {
            if let Some(func) = self.func.lock().unwrap().take() {
                func();
            }
        }
    }
}

/// Whether the caller is on the CEF UI thread.
pub fn on_ui_thread() -> bool {
    currently_on(ThreadId::UI) != 0
}

/// Runs `func` on the CEF UI thread — immediately if already on it, otherwise by
/// posting a task. CEF window/browser operations must run on the UI thread or
/// they are silently ignored.
pub fn post_on_ui(func: impl FnOnce() + Send + 'static) {
    if on_ui_thread() {
        func();
        return;
    }
    let boxed: Box<dyn FnOnce() + Send> = Box::new(func);
    let mut task = CefTask::new(Arc::new(Mutex::new(Some(boxed))));
    post_task(ThreadId::UI, Some(&mut task));
}

/// Pumps one iteration of the CEF message loop. Call from the host (tao) event
/// loop so CEF can process its work without owning the loop via
/// [`run_message_loop`].
pub fn pump() {
    do_message_loop_work();
}

/// Shuts CEF down cleanly. Call once, after the event loop ends.
///
/// Closes the browser and pumps the message loop until it is fully destroyed
/// (`on_before_close`), so `shutdown()` doesn't abort on `observers_.empty()`.
/// Initiates a graceful close of the top-level window (cascades to the browser
/// view + browser). Call on the UI thread while the event loop is still running
/// so GTK/CEF can process it; the browser closes asynchronously — poll
/// [`browser_closed`].
pub fn request_close() {
    let window = GLOBAL_WINDOW.lock().unwrap().clone();
    if let Some(window) = window {
        window.close();
    } else if let Some(browser) = GLOBAL_BROWSER.lock().unwrap().clone() {
        if let Some(host) = browser.host() {
            host.close_browser(1);
        }
    }
}

pub fn shutdown_cef() {
    // Close the top-level window (cascades to the browser view + browser),
    // falling back to closing the browser directly.
    let window = GLOBAL_WINDOW.lock().unwrap().take();
    if let Some(window) = window {
        window.close();
    } else if let Some(browser) = GLOBAL_BROWSER.lock().unwrap().clone() {
        if let Some(host) = browser.host() {
            host.close_browser(1);
        }
    }

    // Pump until the browser has fully torn down, so `shutdown()` doesn't abort
    // on `observers_.empty()`.
    for _ in 0..200 {
        if BROWSER_CLOSED.load(Ordering::SeqCst) {
            break;
        }
        do_message_loop_work();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    shutdown();
}
