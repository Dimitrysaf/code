//! CEF `ipc://` transport for Tauri's invoke bridge.
//!
//! The injected invoke script issues `fetch("ipc://localhost/<cmd>", { headers })`
//! with the invoke payload in the `Tauri-CefRuntime-Invoke-Body` header. We
//! register `ipc` as a fetchable custom scheme plus a scheme handler that turns
//! each request into an `http::Request`, dispatches it through the stored Tauri
//! handler (which hops to the main thread and runs Tauri's invoke handler), and
//! streams the response body back to Chromium.

use std::sync::{Arc, Mutex};

use cef::*;

use crate::backend::WebResourceResponseFn;

type IpcHandler =
    Box<dyn FnMut(http::Request<Vec<u8>>, WebResourceResponseFn) + Send>;

static IPC_HANDLER: Mutex<Option<Arc<Mutex<IpcHandler>>>> = Mutex::new(None);

/// Stores the Tauri IPC dispatch closure built in `create_webview`.
pub fn set_ipc_handler(handler: IpcHandler) {
    *IPC_HANDLER.lock().unwrap() = Some(Arc::new(Mutex::new(handler)));
}

/// Headers the Tauri IPC layer needs copied from the fetch request.
const IPC_HEADERS: &[&str] = &[
    "Content-Type",
    "Origin",
    "Tauri-Callback",
    "Tauri-Error",
    "Tauri-Invoke-Key",
    "Tauri-CefRuntime-Invoke-Body",
];

#[derive(Default)]
struct ResponseState {
    status: u16,
    mime: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    read_offset: usize,
}

wrap_scheme_handler_factory! {
    pub struct IpcSchemeHandlerFactory {}

    impl SchemeHandlerFactory {
        fn create(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            _request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            Some(IpcResourceHandler::new(Arc::new(Mutex::new(
                ResponseState::default(),
            ))))
        }
    }
}

wrap_resource_handler! {
    pub struct IpcResourceHandler {
        state: Arc<Mutex<ResponseState>>,
    }

    impl ResourceHandler {
        fn open(
            &self,
            request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            let Some(request) = request else {
                return 0;
            };
            let url = CefString::from(&request.url()).to_string();
            let method = CefString::from(&request.method()).to_string();

            let mut builder = http::Request::builder().uri(url.as_str());
            if let Ok(method) = http::Method::from_bytes(method.as_bytes()) {
                builder = builder.method(method);
            }
            for name in IPC_HEADERS {
                let value = CefString::from(
                    &request.header_by_name(Some(&CefString::from(*name))),
                )
                .to_string();
                if !value.is_empty() {
                    builder = builder.header(*name, value);
                }
            }
            let Ok(http_request) = builder.body(Vec::new()) else {
                return 0;
            };

            let Some(handler) = IPC_HANDLER.lock().unwrap().clone() else {
                return 0;
            };
            let state = self.state.clone();
            let callback = callback.map(|callback| callback.clone());
            let response_fn: WebResourceResponseFn = Box::new(move |response| {
                {
                    let mut state = state.lock().unwrap();
                    if let Some(response) = response {
                        state.status = response.status().as_u16();
                        state.mime = response
                            .headers()
                            .get(http::header::CONTENT_TYPE)
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or("application/octet-stream")
                            .to_string();
                        for (name, value) in response.headers().iter() {
                            if let Ok(value) = value.to_str() {
                                state
                                    .headers
                                    .push((name.as_str().to_string(), value.to_string()));
                            }
                        }
                        state.body = response.into_body();
                    } else {
                        state.status = 404;
                    }
                }
                if let Some(callback) = &callback {
                    callback.cont();
                }
            });
            (handler.lock().unwrap())(http_request, response_fn);

            // Handled asynchronously: the response arrives via `callback.cont()`.
            if let Some(handle_request) = handle_request {
                *handle_request = 0;
            }
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let state = self.state.lock().unwrap();
            if let Some(response) = response {
                response.set_status(state.status as ::std::os::raw::c_int);
                response.set_mime_type(Some(&CefString::from(state.mime.as_str())));
                for (name, value) in &state.headers {
                    if name.eq_ignore_ascii_case("content-type") {
                        continue;
                    }
                    response.set_header_by_name(
                        Some(&CefString::from(name.as_str())),
                        Some(&CefString::from(value.as_str())),
                        1,
                    );
                }
            }
            if let Some(response_length) = response_length {
                *response_length = state.body.len() as i64;
            }
        }

        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let mut state = self.state.lock().unwrap();
            let remaining = state.body.len().saturating_sub(state.read_offset);
            if remaining == 0 {
                if let Some(bytes_read) = bytes_read {
                    *bytes_read = 0;
                }
                return 0;
            }
            let count = remaining.min(bytes_to_read.max(0) as usize);
            unsafe {
                std::ptr::copy_nonoverlapping(
                    state.body[state.read_offset..].as_ptr(),
                    data_out,
                    count,
                );
            }
            state.read_offset += count;
            if let Some(bytes_read) = bytes_read {
                *bytes_read = count as ::std::os::raw::c_int;
            }
            1
        }
    }
}

/// Registers the `ipc` scheme handler factory. Call once after `initialize`.
pub fn register_ipc_scheme_factory() {
    let mut factory = IpcSchemeHandlerFactory::new();
    register_scheme_handler_factory(
        Some(&CefString::from("ipc")),
        None,
        Some(&mut factory),
    );
}
