//! The window, the Servo instance and everything that glues the sidebar UI to the content
//! webviews. One window for now.
//!
//! Rendering: every webview (the sidebar UI and each visible tab) paints into its own offscreen
//! framebuffer, then we blit them into the window, UI first as the backdrop and the panes on top.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dpi::PhysicalSize;
use euclid::Scale;
use euclid::default::{Point2D, Rect as GlRect, Size2D};
use servo::protocol_handler::ProtocolRegistry;
use servo::{
    Cursor, EmbedderControl, EventLoopWaker, ImeEvent, InputEvent, LoadStatus, MouseButton,
    MouseButtonAction, MouseButtonEvent, MouseLeftViewportEvent, MouseMoveEvent,
    OffscreenRenderingContext, Opts, RenderingContext, Servo, ServoBuilder, Theme, WebView,
    WebViewBuilder, WebViewDelegate, WebViewId, WheelDelta, WheelEvent, WheelMode,
    WindowRenderingContext,
};
use url::Url;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

use crate::bridge::{Command, SplitView, UiState, UpdateView};
use crate::dialogs::Dialogs;
use crate::favicon;
use crate::keys::{self, Action};
use crate::keyutils::keyboard_event_from_winit;
use crate::layout::{self, Rect};
use crate::omnibox;
use crate::protocol::{self, DriftProtocol, NEW_TAB_URL, UiWebViewId};
use crate::session::{self, Session};
use crate::tabs::{Side, TabId, Tabs};
use crate::updater;

const LINE_HEIGHT: f32 = 76.0;
const SESSION_SAVE_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub struct Wake;

#[derive(Clone)]
struct Waker(EventLoopProxy<Wake>);

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        let _ = self.0.send_event(Wake);
    }
}

pub struct Config {
    pub url: Option<String>,
    pub session_path: Option<PathBuf>,
    pub profile_dir: Option<PathBuf>,
    pub restore: bool,
    /// Dev and docs helper: save the window as a PNG once the page settles, then quit.
    pub screenshot: Option<PathBuf>,
    /// Forced theme, None follows the desktop.
    pub theme: Option<crate::theme::Preference>,
    /// Look for a newer release on GitHub at most once a day.
    pub update_check: bool,
}

pub struct App {
    config: Config,
    proxy: EventLoopProxy<Wake>,
    running: Option<Rc<Running>>,
    relaunch: Option<PathBuf>,
}

impl App {
    pub fn new(config: Config, proxy: EventLoopProxy<Wake>) -> Self {
        Self {
            config,
            proxy,
            running: None,
            relaunch: None,
        }
    }

    /// Set when the user asked to restart into a freshly installed update.
    pub fn relaunch(&self) -> Option<&PathBuf> {
        self.relaunch.as_ref()
    }
}

/// Which webview a pointer or keyboard event belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Ui,
    Pane(usize),
}

struct TabView {
    webview: WebView,
    context: Rc<OffscreenRenderingContext>,
}

struct State {
    tabs: Tabs,
    views: HashMap<TabId, TabView>,
    by_webview: HashMap<WebViewId, TabId>,
    content: Rect,
    panes: Vec<Rect>,
    mouse: (f64, f64),
    modifiers: ModifiersState,
    capture: Option<Target>,
    hover: Option<Target>,
    keyboard: Target,
    ui_ready: bool,
    ui_dirty: bool,
    immersive: bool,
    last_session: String,
    last_session_save: Instant,
}

/// Field order is drop order and it matters here: webviews go first, then Servo (whose drop
/// spins until the constellation is gone), then the GL context, and the window last.
/// Dropping the window under a live GL context segfaults on exit.
struct Running {
    dialogs: RefCell<Dialogs>,
    state: RefCell<State>,
    ui: WebView,
    ui_context: Rc<OffscreenRenderingContext>,
    servo: Servo,
    gl: Rc<WindowRenderingContext>,
    window: Window,
    commands: Receiver<Command>,
    session_path: Option<PathBuf>,
    closing: Cell<bool>,
    screenshot: Option<PathBuf>,
    started: Instant,
    last_load_change: Cell<Instant>,
    theme: Cell<Theme>,
    theme_applied: Cell<bool>,
    last_ui_push: Cell<Instant>,
    forced_theme: Option<crate::theme::Preference>,
    waker: Waker,
    update_target: Option<updater::Target>,
    update_sender: Sender<updater::Event>,
    update_events: Receiver<updater::Event>,
    update: RefCell<Option<UpdateView>>,
    release: RefCell<Option<updater::Release>>,
    restart: Cell<bool>,
}

impl ApplicationHandler<Wake> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match Running::start(event_loop, &self.config, self.proxy.clone()) {
            Ok(running) => self.running = Some(running),
            Err(err) => {
                log::error!("could not start: {err}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: Wake) {
        if let Some(running) = &self.running {
            running.pump(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let Some(running) = &self.running {
            running.handle_window_event(event_loop, event);
            running.pump(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // the screenshot helper needs frames even when nothing happens, so keep ticking
        if let Some(running) = &self.running
            && running.screenshot.is_some()
        {
            running.window.request_redraw();
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(250),
            ));
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let Some(running) = self.running.take() {
            running.save_session(true);
            if running.restart.get() {
                self.relaunch = running.update_target.as_ref().map(|t| t.path().to_owned());
            }
            // tear down while the event loop and display connection still exist
            drop(running);
        }
    }
}

impl Running {
    fn start(
        event_loop: &ActiveEventLoop,
        config: &Config,
        proxy: EventLoopProxy<Wake>,
    ) -> Result<Rc<Self>, String> {
        let attrs = Window::default_attributes()
            .with_title("Drift")
            .with_inner_size(winit::dpi::LogicalSize::new(1360.0, 860.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(560.0, 360.0));
        #[cfg(target_os = "linux")]
        let attrs = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            WindowAttributesExtWayland::with_name(attrs, "dev.drift.Drift", "drift")
        };
        let window = event_loop
            .create_window(attrs)
            .map_err(|e| format!("window: {e}"))?;

        let display = event_loop.display_handle().map_err(|e| e.to_string())?;
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let gl = Rc::new(
            WindowRenderingContext::new(display, handle, size)
                .map_err(|e| format!("GL context: {e:?}"))?,
        );
        gl.make_current()
            .map_err(|e| format!("GL current: {e:?}"))?;

        let waker = Waker(proxy);
        let (update_tx, update_rx) = channel();
        let update_target = updater::Target::detect();
        if config.update_check
            && let Some(target) = update_target.clone()
        {
            updater::spawn_check(target, update_tx.clone(), Box::new(waker.clone()));
        }
        let (tx, rx) = channel();
        let ui_id: UiWebViewId = Arc::new(Mutex::new(None));
        let mut protocols = ProtocolRegistry::default();
        let _ = protocols.register(
            protocol::SCHEME,
            DriftProtocol::new(tx, Box::new(waker.clone()), ui_id.clone()),
        );

        let opts = Opts {
            config_dir: config.profile_dir.clone(),
            ..Opts::default()
        };
        let servo = ServoBuilder::default()
            .opts(opts)
            .protocol_registry(protocols)
            .event_loop_waker(Box::new(waker.clone()))
            .build();
        servo.setup_logging();

        let scale = window.scale_factor() as f32;
        let content = layout::content_rect(size.width, size.height, scale);

        let tabs = if config.restore {
            config
                .session_path
                .as_deref()
                .and_then(Session::load)
                .map(|s| s.restore())
                .unwrap_or_default()
        } else {
            Tabs::default()
        };

        let ui_context = Rc::new(gl.offscreen_context(size));
        let running = Rc::new_cyclic(|weak: &std::rc::Weak<Running>| {
            let ui = WebViewBuilder::new(&servo, ui_context.clone())
                .url(Url::parse(protocol::UI_URL).expect("static url"))
                .hidpi_scale_factor(Scale::new(scale))
                .delegate(Rc::new(DelegateProxy(weak.clone())))
                .build();
            Running {
                window,
                servo,
                gl,
                ui,
                ui_context,
                commands: rx,
                session_path: config.session_path.clone(),
                dialogs: RefCell::new(Dialogs::default()),
                state: RefCell::new(State {
                    tabs,
                    views: HashMap::new(),
                    by_webview: HashMap::new(),
                    content,
                    panes: vec![content],
                    mouse: (0.0, 0.0),
                    modifiers: ModifiersState::empty(),
                    capture: None,
                    hover: None,
                    keyboard: Target::Pane(0),
                    ui_ready: false,
                    ui_dirty: true,
                    immersive: false,
                    last_session: String::new(),
                    last_session_save: Instant::now(),
                }),
                closing: Cell::new(false),
                screenshot: config.screenshot.clone(),
                forced_theme: config.theme,
                waker: waker.clone(),
                update_target: update_target.clone(),
                update_sender: update_tx.clone(),
                update_events: update_rx,
                update: RefCell::new(None),
                release: RefCell::new(None),
                restart: Cell::new(false),
                started: Instant::now(),
                last_load_change: Cell::new(Instant::now()),
                theme: Cell::new(Theme::Light),
                theme_applied: Cell::new(false),
                last_ui_push: Cell::new(Instant::now()),
            }
        });
        *ui_id.lock().map_err(|_| "poisoned")? = Some(running.ui.id());
        running.window.set_ime_allowed(true);
        running.ui.show();
        running.apply_theme(running.window.theme());

        {
            let mut state = running.state.borrow_mut();
            match &config.url {
                Some(input) => {
                    let url = omnibox::resolve(input)
                        .map(String::from)
                        .unwrap_or_else(|| NEW_TAB_URL.into());
                    state.tabs.open(url);
                }
                None if state.tabs.is_empty() => {
                    state.tabs.open(NEW_TAB_URL);
                }
                None => {}
            }
        }
        running.sync_views();
        Ok(running)
    }

    fn delegate(self: &Rc<Self>) -> Rc<dyn WebViewDelegate> {
        Rc::new(DelegateProxy(Rc::downgrade(self)))
    }

    fn scale(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    /// Runs Servo's work, applies queued UI commands and pushes state to the sidebar.
    fn pump(self: &Rc<Self>, event_loop: &ActiveEventLoop) {
        self.servo.spin_event_loop();
        while let Ok(command) = self.commands.try_recv() {
            self.handle_command(command);
        }
        while let Ok(event) = self.update_events.try_recv() {
            self.handle_update_event(event);
        }
        self.flush_ui();
        self.save_session(false);
        if self.closing.get() {
            event_loop.exit();
        }
    }

    // ---- layout and views -------------------------------------------------------------------

    fn relayout(&self) {
        let size = self.window.inner_size();
        let scale = self.scale();
        let mut state = self.state.borrow_mut();
        let content = if state.immersive {
            Rect::new(0, 0, size.width as i32, size.height as i32)
        } else {
            layout::content_rect(size.width, size.height, scale)
        };
        let ratio = state.tabs.split().map(|s| s.ratio);
        state.content = content;
        state.panes = layout::panes(content, ratio, scale);
    }

    /// Makes the webviews match the tab model: creates missing ones, shows and sizes the visible
    /// ones, hides the rest so Servo can throttle them.
    fn sync_views(self: &Rc<Self>) {
        self.relayout();
        let visible = self.state.borrow().tabs.visible();
        let panes = self.state.borrow().panes.clone();

        for (index, id) in visible.iter().enumerate() {
            let Some(rect) = panes.get(index).copied() else {
                continue;
            };
            let size = PhysicalSize::new(rect.w.max(1) as u32, rect.h.max(1) as u32);
            if !self.state.borrow().views.contains_key(id) {
                self.create_view(*id, size, None);
            }
            let state = self.state.borrow();
            if let Some(view) = state.views.get(id) {
                if view.context.size() != size {
                    view.webview.resize(size);
                }
                view.webview.show();
            }
        }

        {
            let state = self.state.borrow();
            for (id, view) in &state.views {
                if !visible.contains(id) {
                    view.webview.hide();
                }
            }
        }

        self.sync_focus();
        self.mark_ui_dirty();
        self.window.request_redraw();
    }

    fn create_view(
        self: &Rc<Self>,
        id: TabId,
        size: PhysicalSize<u32>,
        request: Option<servo::CreateNewWebViewRequest>,
    ) -> Option<WebView> {
        let context = Rc::new(self.gl.offscreen_context(size));
        let builder = match request {
            Some(request) => request.builder(context.clone()),
            None => {
                let url = self
                    .state
                    .borrow()
                    .tabs
                    .get(id)
                    .and_then(|t| Url::parse(&t.url).ok())
                    .unwrap_or_else(|| Url::parse(NEW_TAB_URL).expect("static url"));
                WebViewBuilder::new(&self.servo, context.clone()).url(url)
            }
        };
        let webview = builder
            .hidpi_scale_factor(Scale::new(self.scale()))
            .delegate(self.delegate())
            .build();
        webview.notify_theme_change(self.theme.get());

        let mut state = self.state.borrow_mut();
        state.by_webview.insert(webview.id(), id);
        state.views.insert(
            id,
            TabView {
                webview: webview.clone(),
                context,
            },
        );
        Some(webview)
    }

    fn sync_focus(&self) {
        let state = self.state.borrow();
        let focused_tab = state.tabs.focused();
        for (id, view) in &state.views {
            let has_keys = Some(*id) == focused_tab && state.keyboard != Target::Ui;
            if view.webview.focused() != has_keys {
                view.webview.set_focused(has_keys);
            }
        }
        let ui_keys = state.keyboard == Target::Ui;
        if self.ui.focused() != ui_keys {
            self.ui.set_focused(ui_keys);
        }
    }

    fn focused_webview(&self) -> Option<WebView> {
        let state = self.state.borrow();
        let id = state.tabs.focused()?;
        state.views.get(&id).map(|v| v.webview.clone())
    }

    fn webview_for(&self, target: Target) -> Option<WebView> {
        match target {
            Target::Ui => Some(self.ui.clone()),
            Target::Pane(index) => {
                let state = self.state.borrow();
                let id = *state.tabs.visible().get(index)?;
                state.views.get(&id).map(|v| v.webview.clone())
            }
        }
    }

    fn tab_of(&self, webview: &WebView) -> Option<TabId> {
        self.state.borrow().by_webview.get(&webview.id()).copied()
    }

    fn hit_test(&self, x: f64, y: f64) -> Target {
        let state = self.state.borrow();
        if self.dialogs.borrow().is_open() {
            return Target::Ui;
        }
        state
            .panes
            .iter()
            .position(|r| r.contains(x, y))
            .map(Target::Pane)
            .unwrap_or(Target::Ui)
    }

    /// Converts a window position to a point inside the target webview.
    fn local_point(&self, target: Target, x: f64, y: f64) -> servo::DevicePoint {
        let (ox, oy) = match target {
            Target::Ui => (0, 0),
            Target::Pane(i) => self
                .state
                .borrow()
                .panes
                .get(i)
                .map(|r| (r.x, r.y))
                .unwrap_or((0, 0)),
        };
        servo::DevicePoint::new((x - ox as f64) as f32, (y - oy as f64) as f32)
    }

    // ---- rendering --------------------------------------------------------------------------

    fn paint(&self) {
        let state = self.state.borrow();
        let size = self.window.inner_size();
        let height = size.height as i32;

        self.ui.paint();
        let visible = state.tabs.visible();
        let mut blits = Vec::with_capacity(visible.len());
        for (index, id) in visible.iter().enumerate() {
            if let (Some(view), Some(rect)) = (state.views.get(id), state.panes.get(index)) {
                view.webview.paint();
                blits.push((view.context.clone(), *rect));
            }
        }

        if let Err(err) = self.gl.make_current() {
            log::warn!("make_current failed: {err:?}");
            return;
        }
        self.gl.prepare_for_rendering();
        let gl = self.gl.glow_gl_api();

        if let Some(blit) = self.ui_context.render_to_parent_callback() {
            blit(
                &gl,
                GlRect::new(Point2D::new(0, 0), Size2D::new(size.width as i32, height)),
            );
        }
        // in immersive mode the page covers everything, the UI blit above still matters for
        // the frame where we switch, so it stays unconditional
        for (context, rect) in blits {
            if let Some(blit) = context.render_to_parent_callback() {
                let r = rect.to_gl(height);
                blit(
                    &gl,
                    GlRect::new(Point2D::new(r.x, r.y), Size2D::new(r.w, r.h)),
                );
            }
        }
        if self.screenshot_ready() {
            self.take_screenshot(size);
        }
        self.gl.present();
    }

    /// Waits for the focused page to finish loading plus a beat for fonts and images,
    /// with a hard cap so a page that never settles still gets captured.
    fn screenshot_ready(&self) -> bool {
        if self.screenshot.is_none() || self.closing.get() {
            return false;
        }
        let elapsed = self.started.elapsed();
        let state = self.state.borrow();
        let loading = state
            .tabs
            .visible()
            .iter()
            .any(|id| state.tabs.get(*id).is_none_or(|t| t.loading));
        // the sidebar repaints a frame after its update script runs, give it a moment too
        let settled = self.last_load_change.get().elapsed() > Duration::from_millis(2500)
            && self.last_ui_push.get().elapsed() > Duration::from_millis(800);
        (state.ui_ready && !loading && settled && elapsed > Duration::from_secs(4))
            || elapsed > Duration::from_secs(30)
    }

    fn take_screenshot(&self, size: PhysicalSize<u32>) {
        let Some(path) = &self.screenshot else { return };
        let rect = servo::DeviceIntRect::from_size(servo::DeviceIntSize::new(
            size.width as i32,
            size.height as i32,
        ));
        match self.gl.read_to_image(rect) {
            Some(image) => match image.save(path) {
                Ok(()) => log::info!("saved screenshot to {}", path.display()),
                Err(err) => log::error!("could not save screenshot: {err}"),
            },
            None => log::error!("could not read the window framebuffer"),
        }
        self.closing.set(true);
    }

    // ---- window events ----------------------------------------------------------------------

    fn handle_window_event(self: &Rc<Self>, event_loop: &ActiveEventLoop, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => self.paint(),
            WindowEvent::CloseRequested => {
                self.save_session(true);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return;
                }
                self.gl.resize(size);
                self.ui.resize(size);
                self.sync_views();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let scale = Scale::new(scale_factor as f32);
                self.ui.set_hidpi_scale_factor(scale);
                for view in self.state.borrow().views.values() {
                    view.webview.set_hidpi_scale_factor(scale);
                }
                self.sync_views();
            }
            WindowEvent::ThemeChanged(theme) => self.apply_theme(Some(theme)),
            WindowEvent::ModifiersChanged(m) => self.state.borrow_mut().modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event),
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_mouse_move(position.x, position.y)
            }
            WindowEvent::CursorLeft { .. } => {
                let hover = self.state.borrow_mut().hover.take();
                if let Some(webview) = hover.and_then(|t| self.webview_for(t)) {
                    webview.notify_input_event(InputEvent::MouseLeftViewport(
                        MouseLeftViewportEvent::default(),
                    ));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_button(state, button)
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = self.state.borrow().mouse;
                let target = self.hit_test(x, y);
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => {
                        ((dx * LINE_HEIGHT) as f64, (dy * LINE_HEIGHT) as f64)
                    }
                    MouseScrollDelta::PixelDelta(d) => (d.x, d.y),
                };
                if let Some(webview) = self.webview_for(target) {
                    let point = self.local_point(target, x, y);
                    webview.notify_input_event(InputEvent::Wheel(WheelEvent::new(
                        WheelDelta {
                            x: dx,
                            y: dy,
                            z: 0.0,
                            mode: WheelMode::DeltaPixel,
                        },
                        point.into(),
                    )));
                }
            }
            WindowEvent::Ime(ime) => self.handle_ime(ime),
            WindowEvent::DroppedFile(path) => {
                if let (Ok(url), Some(webview)) =
                    (Url::from_file_path(&path), self.focused_webview())
                {
                    webview.load(url);
                }
            }
            // there's no portal change signal hooked up, so re-check when the user comes back,
            // that's when a theme switch made elsewhere would be noticed anyway
            WindowEvent::Focused(true) => self.apply_theme(self.window.theme()),
            WindowEvent::Focused(false) => {
                // dropping a drag halfway because the window lost focus is better than a stuck capture
                self.state.borrow_mut().capture = None;
            }
            _ => {}
        }
    }

    fn handle_key(self: &Rc<Self>, event: winit::event::KeyEvent) {
        let modifiers = self.state.borrow().modifiers;
        let keyboard_event = keyboard_event_from_winit(&event, modifiers);

        if let Some(action) = keys::shortcut(&keyboard_event) {
            self.run_action(action);
            return;
        }
        if self.dialogs.borrow().is_open() {
            // modal dialogs live in the UI webview, keys go there no matter what was focused
            self.ui
                .notify_input_event(InputEvent::Keyboard(keyboard_event));
            return;
        }
        let target = self.state.borrow().keyboard;
        let webview = match target {
            Target::Ui => Some(self.ui.clone()),
            Target::Pane(_) => self.focused_webview(),
        };
        if let Some(webview) = webview {
            webview.notify_input_event(InputEvent::Keyboard(keyboard_event));
        }
    }

    fn handle_mouse_move(&self, x: f64, y: f64) {
        let capture = {
            let mut state = self.state.borrow_mut();
            state.mouse = (x, y);
            state.capture
        };
        let target = capture.unwrap_or_else(|| self.hit_test(x, y));
        let previous = self.state.borrow_mut().hover.replace(target);
        if let Some(previous) = previous.filter(|p| *p != target)
            && let Some(webview) = self.webview_for(previous)
        {
            webview.notify_input_event(InputEvent::MouseLeftViewport(
                MouseLeftViewportEvent::default(),
            ));
        }
        if let Some(webview) = self.webview_for(target) {
            if previous != Some(target) {
                self.window.set_cursor(cursor_icon(webview.cursor()));
            }
            let point = self.local_point(target, x, y);
            webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(point.into())));
        }
    }

    fn handle_mouse_button(
        self: &Rc<Self>,
        state: ElementState,
        button: winit::event::MouseButton,
    ) {
        let (x, y) = self.state.borrow().mouse;
        let target = match state {
            ElementState::Pressed => {
                let target = self.hit_test(x, y);
                self.state.borrow_mut().capture = Some(target);
                if button == winit::event::MouseButton::Left {
                    self.focus_target(target);
                }
                target
            }
            ElementState::Released => self
                .state
                .borrow_mut()
                .capture
                .take()
                .unwrap_or_else(|| self.hit_test(x, y)),
        };

        // back and forward buttons on the mouse act on the page under the pointer
        if state == ElementState::Pressed {
            match button {
                winit::event::MouseButton::Back => {
                    self.run_action(Action::Back);
                    return;
                }
                winit::event::MouseButton::Forward => {
                    self.run_action(Action::Forward);
                    return;
                }
                _ => {}
            }
        }

        let servo_button = match button {
            winit::event::MouseButton::Left => MouseButton::Primary,
            winit::event::MouseButton::Right => MouseButton::Secondary,
            winit::event::MouseButton::Middle => MouseButton::Auxiliary,
            winit::event::MouseButton::Back => MouseButton::Back,
            winit::event::MouseButton::Forward => MouseButton::Forward,
            winit::event::MouseButton::Other(v) => MouseButton::Other(v),
        };
        let action = match state {
            ElementState::Pressed => MouseButtonAction::Down,
            ElementState::Released => MouseButtonAction::Up,
        };
        if let Some(webview) = self.webview_for(target) {
            let point = self.local_point(target, x, y);
            webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                action,
                servo_button,
                point.into(),
            )));
        }
    }

    fn focus_target(self: &Rc<Self>, target: Target) {
        {
            let mut state = self.state.borrow_mut();
            state.keyboard = target;
            if let Target::Pane(index) = target
                && state.tabs.split().is_some()
            {
                state
                    .tabs
                    .focus_side(if index == 0 { Side::Left } else { Side::Right });
            }
        }
        self.sync_focus();
        self.mark_ui_dirty();
    }

    fn handle_ime(&self, ime: Ime) {
        let target = self.state.borrow().keyboard;
        let webview = match target {
            Target::Ui => Some(self.ui.clone()),
            Target::Pane(_) => self.focused_webview(),
        };
        let Some(webview) = webview else { return };
        let composition = |state, data| {
            InputEvent::Ime(ImeEvent::Composition(servo::CompositionEvent {
                state,
                data,
            }))
        };
        match ime {
            Ime::Enabled => {
                webview
                    .notify_input_event(composition(servo::CompositionState::Start, String::new()));
            }
            Ime::Preedit(text, _) => {
                webview.notify_input_event(composition(servo::CompositionState::Update, text));
            }
            Ime::Commit(text) => {
                webview
                    .notify_input_event(composition(servo::CompositionState::Update, text.clone()));
                webview.notify_input_event(composition(servo::CompositionState::End, text));
            }
            Ime::Disabled => {}
        }
    }

    /// Pushes the desktop's light/dark choice into every webview, so `prefers-color-scheme`
    /// works for the UI and for pages. Cheap to call again, it only notifies on a change.
    fn apply_theme(&self, reported: Option<winit::window::Theme>) {
        let theme = match self.forced_theme {
            Some(crate::theme::Preference::Dark) => Theme::Dark,
            Some(crate::theme::Preference::Light) => Theme::Light,
            None => resolve_theme(reported),
        };
        let previous = self.theme.replace(theme);
        let first = !self.theme_applied.replace(true);
        if previous == theme && !first {
            return;
        }
        self.ui.notify_theme_change(theme);
        for view in self.state.borrow().views.values() {
            view.webview.notify_theme_change(theme);
        }
    }

    // ---- actions and commands ---------------------------------------------------------------

    fn run_action(self: &Rc<Self>, action: Action) {
        match action {
            Action::NewTab => self.handle_command(Command::NewTab),
            Action::CloseTab => {
                let id = self.state.borrow().tabs.focused();
                if let Some(id) = id {
                    self.handle_command(Command::Close { id });
                }
            }
            Action::FocusAddress => self.focus_address(),
            Action::NextTab | Action::PrevTab => {
                self.state
                    .borrow_mut()
                    .tabs
                    .cycle(action == Action::NextTab);
                self.state.borrow_mut().keyboard = Target::Pane(0);
                self.sync_views();
            }
            Action::ToggleSplit => self.handle_command(Command::ToggleSplit),
            Action::Back => self.handle_command(Command::Back),
            Action::Forward => self.handle_command(Command::Forward),
            Action::Reload => self.handle_command(Command::Reload),
            Action::ZoomIn | Action::ZoomOut | Action::ZoomReset => {
                if let Some(webview) = self.focused_webview() {
                    let zoom = match action {
                        Action::ZoomIn => (webview.page_zoom() + 0.1).min(5.0),
                        Action::ZoomOut => (webview.page_zoom() - 0.1).max(0.3),
                        _ => 1.0,
                    };
                    webview.set_page_zoom(zoom);
                }
            }
            Action::Fullscreen => {
                let on = self.window.fullscreen().is_none();
                self.window
                    .set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
            }
        }
    }

    fn focus_address(self: &Rc<Self>) {
        self.focus_target(Target::Ui);
        self.ui
            .evaluate_javascript("window.drift && window.drift.focusAddress()", |_| {});
    }

    fn handle_command(self: &Rc<Self>, command: Command) {
        log::debug!("command {command:?}");
        match command {
            Command::Ready => {
                self.state.borrow_mut().ui_ready = true;
                self.mark_ui_dirty();
                let empty = self
                    .state
                    .borrow()
                    .tabs
                    .get(self.state.borrow().tabs.focused().unwrap_or_default())
                    .is_some_and(|t| t.url == NEW_TAB_URL);
                if empty {
                    self.focus_address();
                }
            }
            Command::NewTab => {
                self.state.borrow_mut().tabs.open(NEW_TAB_URL);
                self.sync_views();
                self.focus_address();
            }
            Command::Close { id } => self.close_tab(id),
            Command::Activate { id } => {
                let changed = self.state.borrow_mut().tabs.activate(id);
                if changed {
                    self.state.borrow_mut().keyboard = Target::Pane(0);
                    self.sync_views();
                }
            }
            Command::Navigate { input } => {
                let Some(url) = omnibox::resolve(&input) else {
                    return;
                };
                if self.state.borrow().tabs.focused().is_none() {
                    self.state.borrow_mut().tabs.open(url.as_str());
                    self.sync_views();
                } else if let Some(webview) = self.focused_webview() {
                    webview.load(url);
                }
                self.state.borrow_mut().keyboard = Target::Pane(0);
                self.sync_focus();
            }
            Command::Back => {
                if let Some(w) = self.focused_webview().filter(|w| w.can_go_back()) {
                    w.go_back(1);
                }
            }
            Command::Forward => {
                if let Some(w) = self.focused_webview().filter(|w| w.can_go_forward()) {
                    w.go_forward(1);
                }
            }
            Command::Reload => {
                if let Some(w) = self.focused_webview() {
                    w.reload();
                }
            }
            Command::ToggleSplit => {
                let is_split = self.state.borrow().tabs.split().is_some();
                if is_split {
                    self.state.borrow_mut().tabs.unsplit();
                } else {
                    let mut state = self.state.borrow_mut();
                    let other = state
                        .tabs
                        .split_candidate()
                        .unwrap_or_else(|| state.tabs.open_in_background(NEW_TAB_URL));
                    state.tabs.split_with(other);
                }
                self.sync_views();
            }
            Command::SplitWith { id } => {
                if self.state.borrow_mut().tabs.split_with(id) {
                    self.sync_views();
                }
            }
            Command::SetRatio { ratio } => {
                self.state.borrow_mut().tabs.set_ratio(ratio);
                self.sync_views();
            }
            Command::FocusPane { side } => {
                self.state.borrow_mut().tabs.focus_side(side);
                self.state.borrow_mut().keyboard = Target::Pane(0);
                self.sync_views();
            }
            Command::MoveTab { id, to } => {
                self.state.borrow_mut().tabs.move_tab(id, to);
                self.mark_ui_dirty();
            }
            Command::FocusContent => {
                self.state.borrow_mut().keyboard = Target::Pane(0);
                self.sync_focus();
            }
            Command::DialogResult { id, ok, value } => {
                self.dialogs.borrow_mut().resolve(id, ok, value);
                self.mark_ui_dirty();
                self.window.request_redraw();
            }
            Command::InstallUpdate => {
                let release = self.release.borrow().clone();
                if let (Some(release), Some(target)) = (release, self.update_target.clone()) {
                    self.set_update_status("downloading", None);
                    updater::spawn_install(
                        release,
                        target,
                        self.update_sender.clone(),
                        Box::new(self.waker.clone()),
                    );
                }
            }
            Command::DismissUpdate => {
                *self.update.borrow_mut() = None;
                self.mark_ui_dirty();
            }
            Command::Restart => {
                self.restart.set(true);
                self.closing.set(true);
            }
        }
    }

    fn handle_update_event(&self, event: updater::Event) {
        match event {
            updater::Event::Available(release) => {
                *self.update.borrow_mut() = Some(UpdateView {
                    version: release.version.clone(),
                    page: release.page.clone(),
                    status: "available",
                    error: None,
                });
                *self.release.borrow_mut() = Some(release);
            }
            updater::Event::Ready => self.set_update_status("ready", None),
            updater::Event::Failed(err) => self.set_update_status("failed", Some(err)),
        }
        self.mark_ui_dirty();
    }

    fn set_update_status(&self, status: &'static str, error: Option<String>) {
        if let Some(view) = self.update.borrow_mut().as_mut() {
            view.status = status;
            view.error = error;
        }
        self.mark_ui_dirty();
    }

    fn close_tab(self: &Rc<Self>, id: TabId) {
        let removed = {
            let mut state = self.state.borrow_mut();
            state.tabs.close(id);
            let view = state.views.remove(&id);
            if let Some(view) = &view {
                state.by_webview.remove(&view.webview.id());
            }
            if state.tabs.is_empty() {
                state.tabs.open(NEW_TAB_URL);
            }
            view
        };
        // dropping the last handle closes the webview, do it after the borrow so delegate
        // callbacks fired during teardown can still look at the state
        drop(removed);
        self.dialogs.borrow_mut().drop_for_tab(id);
        self.sync_views();
    }

    fn mark_ui_dirty(&self) {
        self.state.borrow_mut().ui_dirty = true;
    }

    fn flush_ui(&self) {
        let script = {
            let mut state = self.state.borrow_mut();
            if !state.ui_ready || !state.ui_dirty {
                return;
            }
            state.ui_dirty = false;
            let scale = self.scale();
            let focused = state.tabs.focused();
            let webview = focused
                .and_then(|id| state.views.get(&id))
                .map(|v| &v.webview);
            let url = focused
                .and_then(|id| state.tabs.get(id))
                .map(|t| t.url.clone())
                .filter(|u| u != NEW_TAB_URL)
                .unwrap_or_default();
            let dialogs = self.dialogs.borrow();
            let ui = UiState {
                tabs: state.tabs.all(),
                focused,
                split: state.tabs.split().map(|s| SplitView {
                    left: s.left,
                    right: s.right,
                    ratio: s.ratio,
                    focused: s.focused,
                }),
                url,
                can_go_back: webview.is_some_and(|w| w.can_go_back()),
                can_go_forward: webview.is_some_and(|w| w.can_go_forward()),
                loading: focused
                    .and_then(|id| state.tabs.get(id))
                    .is_some_and(|t| t.loading),
                panes: state.panes.iter().map(|r| r.to_css(scale)).collect(),
                card: state.content.to_css(scale),
                immersive: state.immersive,
                dialog: dialogs.current(),
                update: self.update.borrow().clone(),
            };
            ui.to_script()
        };
        self.last_ui_push.set(Instant::now());
        self.ui.evaluate_javascript(script, |result| {
            if let Err(err) = result {
                log::warn!("UI update failed: {err:?}");
            }
        });
    }

    fn save_session(&self, force: bool) {
        let Some(path) = &self.session_path else {
            return;
        };
        let mut state = self.state.borrow_mut();
        if !force && state.last_session_save.elapsed() < SESSION_SAVE_INTERVAL {
            return;
        }
        state.last_session_save = Instant::now();
        let session = Session::capture(&state.tabs);
        let Ok(json) = serde_json::to_string(&session) else {
            return;
        };
        if json == state.last_session {
            return;
        }
        match session.save(path) {
            Ok(()) => state.last_session = json,
            Err(err) => log::warn!("could not save session: {err}"),
        }
    }

    // ---- delegate callbacks -----------------------------------------------------------------

    fn on_tab<F: FnOnce(&mut crate::tabs::Tab)>(&self, webview: &WebView, f: F) {
        if let Some(id) = self.tab_of(webview) {
            if let Some(tab) = self.state.borrow_mut().tabs.get_mut(id) {
                f(tab);
            }
            self.mark_ui_dirty();
        }
    }
}

/// Webviews hold their delegate forever, a weak handle keeps that from pinning the app alive.
struct DelegateProxy(std::rc::Weak<Running>);

impl DelegateProxy {
    fn app(&self) -> Option<Rc<Running>> {
        self.0.upgrade()
    }
}

impl WebViewDelegate for DelegateProxy {
    fn notify_new_frame_ready(&self, _: WebView) {
        if let Some(app) = self.app() {
            app.window.request_redraw();
        }
    }

    fn notify_url_changed(&self, webview: WebView, url: Url) {
        if let Some(app) = self.app() {
            app.on_tab(&webview, |tab| tab.url = url.to_string());
        }
    }

    fn notify_page_title_changed(&self, webview: WebView, title: Option<String>) {
        if let Some(app) = self.app() {
            app.on_tab(&webview, |tab| tab.title = title.unwrap_or_default());
        }
    }

    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        if let Some(app) = self.app() {
            if webview.id() == app.ui.id() {
                return;
            }
            app.last_load_change.set(Instant::now());
            log::debug!(
                "load status {:?} {status:?}",
                webview.url().map(|u| u.to_string())
            );
            app.on_tab(&webview, |tab| tab.loading = status != LoadStatus::Complete);
        }
    }

    fn notify_favicon_changed(&self, webview: WebView) {
        if let Some(app) = self.app() {
            let icon = webview
                .favicon()
                .and_then(|image| favicon::to_data_url(&image));
            log::debug!(
                "favicon for {:?}: {}",
                webview.url(),
                icon.as_ref().map_or(0, |i| i.len())
            );
            app.on_tab(&webview, |tab| tab.favicon = icon);
        }
    }

    fn notify_history_changed(&self, webview: WebView, _: Vec<Url>, _: usize) {
        if let Some(app) = self.app()
            && app.tab_of(&webview).is_some()
        {
            app.mark_ui_dirty();
        }
    }

    fn notify_cursor_changed(&self, webview: WebView, cursor: Cursor) {
        if let Some(app) = self.app() {
            let hover = app.state.borrow().hover;
            if hover.and_then(|t| app.webview_for(t)).map(|w| w.id()) == Some(webview.id()) {
                app.window.set_cursor(cursor_icon(cursor));
            }
        }
    }

    fn notify_closed(&self, webview: WebView) {
        if let Some(app) = self.app()
            && let Some(id) = app.tab_of(&webview)
        {
            app.close_tab(id);
        }
    }

    fn notify_crashed(&self, webview: WebView, reason: String, _: Option<String>) {
        log::error!("webview crashed: {reason}");
        if let Some(app) = self.app() {
            app.on_tab(&webview, |tab| {
                tab.loading = false;
                tab.title = format!("Crashed: {reason}");
            });
        }
    }

    fn notify_fullscreen_state_changed(&self, _: WebView, fullscreen: bool) {
        if let Some(app) = self.app() {
            app.state.borrow_mut().immersive = fullscreen;
            app.window
                .set_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
            app.sync_views();
        }
    }

    fn request_create_new(&self, parent: WebView, request: servo::CreateNewWebViewRequest) {
        let Some(app) = self.app() else { return };
        let size = app
            .state
            .borrow()
            .panes
            .first()
            .map(|r| PhysicalSize::new(r.w.max(1) as u32, r.h.max(1) as u32))
            .unwrap_or(PhysicalSize::new(800, 600));
        let id = {
            let mut state = app.state.borrow_mut();
            if let Some(parent_id) = state.by_webview.get(&parent.id()).copied() {
                state.tabs.activate(parent_id);
            }
            state.tabs.open("about:blank")
        };
        app.create_view(id, size, Some(request));
        app.state.borrow_mut().keyboard = Target::Pane(0);
        app.sync_views();
    }

    fn show_embedder_control(&self, webview: WebView, control: EmbedderControl) {
        let Some(app) = self.app() else { return };
        let tab = app.tab_of(&webview);
        let origin = webview
            .url()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| "This page".into());
        let offset = {
            let state = app.state.borrow();
            let index = tab.and_then(|id| state.tabs.visible().iter().position(|v| *v == id));
            index
                .and_then(|i| state.panes.get(i))
                .map(|r| (r.x, r.y))
                .unwrap_or((0, 0))
        };
        let scale = app.scale();
        if app
            .dialogs
            .borrow_mut()
            .show(tab, control, origin, offset, scale)
        {
            app.focus_target(Target::Ui);
            app.mark_ui_dirty();
        }
    }

    fn hide_embedder_control(&self, _: WebView, control: servo::EmbedderControlId) {
        if let Some(app) = self.app() {
            app.dialogs.borrow_mut().hide(control);
            app.mark_ui_dirty();
        }
    }
}

fn resolve_theme(reported: Option<winit::window::Theme>) -> Theme {
    match reported {
        Some(winit::window::Theme::Dark) => Theme::Dark,
        Some(winit::window::Theme::Light) => Theme::Light,
        None => match crate::theme::portal_preference() {
            Some(crate::theme::Preference::Dark) => Theme::Dark,
            _ => Theme::Light,
        },
    }
}

fn cursor_icon(cursor: Cursor) -> CursorIcon {
    match cursor {
        Cursor::None | Cursor::Default => CursorIcon::Default,
        Cursor::Pointer => CursorIcon::Pointer,
        Cursor::ContextMenu => CursorIcon::ContextMenu,
        Cursor::Help => CursorIcon::Help,
        Cursor::Progress => CursorIcon::Progress,
        Cursor::Wait => CursorIcon::Wait,
        Cursor::Cell => CursorIcon::Cell,
        Cursor::Crosshair => CursorIcon::Crosshair,
        Cursor::Text => CursorIcon::Text,
        Cursor::VerticalText => CursorIcon::VerticalText,
        Cursor::Alias => CursorIcon::Alias,
        Cursor::Copy => CursorIcon::Copy,
        Cursor::Move => CursorIcon::Move,
        Cursor::NoDrop => CursorIcon::NoDrop,
        Cursor::NotAllowed => CursorIcon::NotAllowed,
        Cursor::Grab => CursorIcon::Grab,
        Cursor::Grabbing => CursorIcon::Grabbing,
        Cursor::EResize => CursorIcon::EResize,
        Cursor::NResize => CursorIcon::NResize,
        Cursor::NeResize => CursorIcon::NeResize,
        Cursor::NwResize => CursorIcon::NwResize,
        Cursor::SResize => CursorIcon::SResize,
        Cursor::SeResize => CursorIcon::SeResize,
        Cursor::SwResize => CursorIcon::SwResize,
        Cursor::WResize => CursorIcon::WResize,
        Cursor::EwResize => CursorIcon::EwResize,
        Cursor::NsResize => CursorIcon::NsResize,
        Cursor::NeswResize => CursorIcon::NeswResize,
        Cursor::NwseResize => CursorIcon::NwseResize,
        Cursor::ColResize => CursorIcon::ColResize,
        Cursor::RowResize => CursorIcon::RowResize,
        Cursor::AllScroll => CursorIcon::AllScroll,
        Cursor::ZoomIn => CursorIcon::ZoomIn,
        Cursor::ZoomOut => CursorIcon::ZoomOut,
    }
}

pub fn default_profile_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("drift").join("profile"))
}

pub fn default_session_path() -> Option<PathBuf> {
    session::default_path()
}
