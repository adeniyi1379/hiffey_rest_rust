//! Shared reactive state, cloned by value into every screen.

use leptos::prelude::*;
use pos_types::{
    LoginResponse, MenuResponse, OrderDto, SettingsDto, StockBalanceDto, SyncStatus, UserSummary,
    WorkerSummary,
};

use crate::api::{Api, ApiError};
use crate::cart::Cart;
use crate::config::{self, Config};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Boot,
    Connection,
    Setup,
    Login,
    Shell,
}

pub const PACKS_CATEGORY: &str = "__packs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Sale,
    Held,
    Stock,
    Menu,
    Workers,
    Settings,
    Reports,
    Sync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub message: String,
}

#[derive(Clone, Copy)]
pub struct App {
    pub config: RwSignal<Config>,
    pub screen: RwSignal<Screen>,
    pub page: RwSignal<Page>,
    pub session: RwSignal<Option<LoginResponse>>,
    pub menu: RwSignal<Option<MenuResponse>>,
    pub settings: RwSignal<Option<SettingsDto>>,
    pub held: RwSignal<Vec<OrderDto>>,
    pub workers: RwSignal<Vec<WorkerSummary>>,
    pub balances: RwSignal<Vec<StockBalanceDto>>,
    pub sync: RwSignal<Option<SyncStatus>>,
    pub cart: RwSignal<Cart>,
    pub selected_category: RwSignal<Option<String>>,
    pub online: RwSignal<bool>,
    pub booted: RwSignal<bool>,
    pub busy: RwSignal<bool>,
    pub connection_error: RwSignal<Option<String>>,
    pub toasts: RwSignal<Vec<Toast>>,
}

impl App {
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: RwSignal::new(Config::default()),
            screen: RwSignal::new(Screen::Boot),
            page: RwSignal::new(Page::Sale),
            session: RwSignal::new(None),
            menu: RwSignal::new(None),
            settings: RwSignal::new(None),
            held: RwSignal::new(Vec::new()),
            workers: RwSignal::new(Vec::new()),
            balances: RwSignal::new(Vec::new()),
            sync: RwSignal::new(None),
            cart: RwSignal::new(Cart::default()),
            selected_category: RwSignal::new(None),
            online: RwSignal::new(false),
            booted: RwSignal::new(false),
            busy: RwSignal::new(false),
            connection_error: RwSignal::new(None),
            toasts: RwSignal::new(Vec::new()),
        }
    }

    #[must_use]
    pub fn user(&self) -> Option<UserSummary> {
        self.session.get().map(|session| session.user)
    }

    #[must_use]
    pub fn role(&self) -> Option<pos_types::Role> {
        self.user().map(|user| user.role)
    }

    #[must_use]
    pub fn api(&self) -> Api {
        let config = self.config.get();
        let token = self.session.get().map(|session| session.token);
        Api::new(&config.base_url, &config.terminal_id, token.as_deref())
    }

    #[must_use]
    pub fn api_anonymous(&self) -> Api {
        let config = self.config.get();
        Api::new(&config.base_url, &config.terminal_id, None)
    }

    pub fn start_session(&self, session: LoginResponse) {
        config::save_token(&session.token);
        self.session.set(Some(session));
        self.online.set(true);
        self.screen.set(Screen::Shell);
    }

    pub fn end_session(&self) {
        config::clear_token();
        self.session.set(None);
        self.menu.set(None);
        self.held.set(Vec::new());
        self.workers.set(Vec::new());
        self.balances.set(Vec::new());
        self.cart.set(Cart::default());
        self.selected_category.set(None);
        self.page.set(Page::Sale);
        self.screen.set(Screen::Login);
    }

    pub fn toast(&self, kind: ToastKind, message: impl Into<String>) {
        let id = next_toast_id();
        self.toasts.update(|toasts| {
            toasts.push(Toast {
                id,
                kind,
                message: message.into(),
            });
        });
    }

    pub fn notify(&self, message: impl Into<String>) {
        self.toast(ToastKind::Success, message);
    }

    /// Shows an API failure; an expired session drops back to the PIN screen.
    pub fn fail(&self, error: &ApiError) {
        if error.is_auth() && self.session.get().is_some() {
            self.end_session();
            self.toast(ToastKind::Error, "Session expired. Enter your PIN again.");
            return;
        }
        if error.is_offline() {
            self.online.set(false);
        }
        self.toast(ToastKind::Error, error.message.clone());
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn next_toast_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub async fn refresh_menu(app: App) {
    if app.session.get().is_none() {
        return;
    }
    match app.api().menu().await {
        Ok(menu) => {
            app.online.set(true);
            let keep = app.selected_category.get();
            let valid = keep.as_ref().is_none_or(|id| {
                id == PACKS_CATEGORY || menu.categories.iter().any(|category| &category.id == id)
            });
            if !valid {
                app.selected_category.set(None);
            }
            app.menu.set(Some(menu));
        }
        Err(error) => app.fail(&error),
    }
}

pub async fn refresh_held(app: App) {
    if app.session.get().is_none() {
        return;
    }
    match app.api().held_orders().await {
        Ok(orders) => {
            app.online.set(true);
            app.held.set(orders);
        }
        Err(error) => app.fail(&error),
    }
}

pub async fn refresh_settings(app: App) {
    if app.session.get().is_none() {
        return;
    }
    match app.api().settings().await {
        Ok(settings) => {
            app.online.set(true);
            app.settings.set(Some(settings));
        }
        Err(error) => app.fail(&error),
    }
}

pub async fn refresh_balances(app: App) {
    if app.session.get().is_none() {
        return;
    }
    match app.api().balances().await {
        Ok(balances) => {
            app.online.set(true);
            app.balances.set(balances);
        }
        Err(error) => app.fail(&error),
    }
}

pub async fn refresh_workers(app: App) {
    if app.session.get().is_none() {
        return;
    }
    match app.api().workers().await {
        Ok(workers) => {
            app.online.set(true);
            app.workers.set(workers);
        }
        Err(error) => app.fail(&error),
    }
}
