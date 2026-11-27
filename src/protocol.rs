//! The `drift://` scheme. `drift://ui/<file>` serves the embedded UI, `drift://ui/bridge?<json>`
//! carries commands from the sidebar to the core.

use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use headers::{ContentType, HeaderMapExt};
use include_dir::{Dir, include_dir};
use servo::protocol_handler::{
    DoneChannel, FetchContext, NetworkError, ProtocolHandler, Request, ResourceFetchTiming,
    Response, ResponseBody,
};
use servo::{EventLoopWaker, WebViewId};

use crate::bridge::Command;

static UI: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/ui");

pub const SCHEME: &str = "drift";
pub const UI_URL: &str = "drift://ui/index.html";
pub const NEW_TAB_URL: &str = "drift://ui/newtab.html";

/// Shared with the handler so it can tell the sidebar apart from web content.
pub type UiWebViewId = Arc<Mutex<Option<WebViewId>>>;

pub struct DriftProtocol {
    commands: Mutex<Sender<Command>>,
    waker: Box<dyn EventLoopWaker>,
    ui_webview: UiWebViewId,
}

impl DriftProtocol {
    pub fn new(
        commands: Sender<Command>,
        waker: Box<dyn EventLoopWaker>,
        ui_webview: UiWebViewId,
    ) -> Self {
        Self {
            commands: Mutex::new(commands),
            waker,
            ui_webview,
        }
    }

    fn bridge(&self, request: &Request, query: Option<&str>) -> Response {
        // yeah, this check is the whole security story of the bridge: web pages can point an
        // <img> or fetch at drift://ui/bridge, but only the sidebar webview may drive the browser
        let from_ui = {
            let ui = self.ui_webview.lock().map(|id| *id).unwrap_or(None);
            ui.is_some() && request.target_webview_id == ui
        };
        if !from_ui {
            log::warn!("rejected bridge call from {:?}", request.target_webview_id);
            return Response::network_error(NetworkError::ResourceLoadError(
                "bridge is only available to the browser UI".into(),
            ));
        }

        match Command::from_query(query.unwrap_or_default()) {
            Ok(command) => {
                if let Ok(tx) = self.commands.lock() {
                    let _ = tx.send(command);
                }
                self.waker.wake();
                body_response(request, ContentType::json(), b"{\"ok\":true}".to_vec())
            }
            Err(err) => {
                log::warn!("bad bridge command: {err}");
                Response::network_error(NetworkError::ResourceLoadError(err))
            }
        }
    }
}

impl ProtocolHandler for DriftProtocol {
    fn is_fetchable(&self) -> bool {
        true
    }

    fn is_secure(&self) -> bool {
        true
    }

    fn load<'a>(
        &'a self,
        request: &'a mut Request,
        _done_chan: &mut DoneChannel,
        _context: &FetchContext,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        let url = request.current_url();
        let response = if url.host_str() != Some("ui") {
            not_found()
        } else if url.path() == "/bridge" {
            self.bridge(request, url.query())
        } else {
            match asset(url.path()) {
                Some((mime, bytes)) => body_response(request, mime, bytes.to_vec()),
                None => not_found(),
            }
        };
        Box::pin(std::future::ready(response))
    }
}

/// Looks up an embedded UI file. `include_dir` only knows what was in ui/ at build time,
/// so there is no way to walk out of it with `..`.
pub fn asset(path: &str) -> Option<(ContentType, &'static [u8])> {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let file = UI.get_file(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some((ContentType::from(mime), file.contents()))
}

fn body_response(request: &Request, mime: ContentType, body: Vec<u8>) -> Response {
    let mut response = Response::new(
        request.current_url(),
        ResourceFetchTiming::new(request.timing_type()),
    );
    response.headers.typed_insert(mime);
    *response.body.lock() = ResponseBody::Done(body);
    response
}

fn not_found() -> Response {
    Response::network_error(NetworkError::ResourceLoadError("not found".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_embedded_ui() {
        let (mime, bytes) = asset("/index.html").unwrap();
        assert_eq!(mime, ContentType::html());
        assert!(!bytes.is_empty());
        assert!(asset("/").is_some());
        assert!(asset("/newtab.html").is_some());
    }

    #[test]
    fn unknown_and_escaping_paths_are_missing() {
        assert!(asset("/nope.html").is_none());
        assert!(asset("/../Cargo.toml").is_none());
        assert!(asset("/../../etc/passwd").is_none());
    }
}
