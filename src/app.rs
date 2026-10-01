use leptos::prelude::*;
use leptos_router::{
    components::{Route, Router, Routes},
    hooks::use_location,
    path,
};

#[cfg(any(feature = "ssr", feature = "hydrate"))]
use crate::browser_scripts;
use crate::ui;

#[must_use]
#[component]
pub(crate) fn App() -> impl IntoView {
    view! {
        <Router>
            <AppRail/>
            <main>
                <Routes fallback=ui::shared::NotFoundPage>
                    <Route path=path!("") view=ui::jobs::JobsPage/>
                    <Route path=path!("archives") view=ui::jobs::ArchivesPage/>
                    <Route path=path!("jobs/new") view=ui::jobs::JobEditorPage/>
                    <Route path=path!("jobs/:id") view=ui::jobs::JobDetailPage/>
                    <Route path=path!("jobs/:id/edit") view=ui::jobs::JobEditorPage/>
                    <Route path=path!("quotes") view=ui::documents::QuotesPage/>
                    <Route path=path!("quotes/new") view=ui::documents::DocumentEditorPage/>
                    <Route path=path!("quotes/:id") view=ui::documents::QuoteDetailPage/>
                    <Route path=path!("quotes/:id/edit") view=ui::documents::DocumentEditorPage/>
                    <Route path=path!("invoices") view=ui::documents::InvoicesPage/>
                    <Route path=path!("invoices/new") view=ui::documents::DocumentEditorPage/>
                    <Route path=path!("invoices/:id/edit") view=ui::documents::DocumentEditorPage/>
                    <Route path=path!("settings") view=ui::settings::SettingsPage/>
                </Routes>
            </main>
            <dialog class="detail-dialog" data-detail-dialog aria-label="Record details"><div class="dialog-toolbar"><button class="secondary-button" type="button" data-dialog-close aria-label="Close details">"Close"</button></div><div data-dialog-content></div></dialog>
        </Router>
    }
}

#[component]
fn AppRail() -> impl IntoView {
    let location = use_location();
    let settings_active = RwSignal::new({
        let pathname = location.pathname.get_untracked();
        pathname == "/settings" || pathname.starts_with("/settings/")
    });
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        let pathname = location.pathname.get();
        settings_active.set(pathname == "/settings" || pathname.starts_with("/settings/"));
    });
    view! {
        <aside class="app-rail" aria-label="Print Manager workspace">
                <a class="rail-brand" href="/" aria-label="Print Manager schedule">
                    <strong>"Print Manager"</strong>
                </a>
                <nav class="workspace-nav" aria-label="Workspace navigation">
                    <AppNavItem href="/" label="Schedule" mark="S" exact=true/>
                    <AppNavItem href="/quotes" label="Quotes" mark="Q"/>
                    <AppNavItem href="/invoices" label="Invoices" mark="$"/>
                    <AppNavItem href="/archives" label="Archives" mark="A"/>
                </nav>
                <div class="rail-actions">
                    <a
                        class="rail-action rail-settings"
                        class:is-active=move || reactive_bool(settings_active)
                        href="/settings"
                        aria-label="Shop settings"
                        aria-current=move || reactive_bool(settings_active).then_some("page")
                    >
                        <svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                            <circle cx="12" cy="12" r="3"/>
                            <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V21h-4v-.08A1.7 1.7 0 0 0 8.95 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15 1.7 1.7 0 0 0 3.07 14H3v-4h.08A1.7 1.7 0 0 0 4.6 8.95a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 8.95 4.6 1.7 1.7 0 0 0 10 3.07V3h4v.08a1.7 1.7 0 0 0 1.03 1.53 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 8.95 1.7 1.7 0 0 0 20.93 10H21v4h-.08A1.7 1.7 0 0 0 19.4 15Z"/>
                        </svg>
                        <span>"Settings"</span>
                    </a>
                    <button class="theme-toggle" type="button" role="switch" aria-checked="false" aria-label="Switch to dark mode" data-theme-toggle>
                        <span class="theme-toggle-track" aria-hidden="true">
                            <svg class="theme-toggle-glyph theme-toggle-sun" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                                <circle cx="12" cy="12" r="3.5"/>
                                <path d="M12 2.5v2M12 19.5v2M4.93 4.93l1.42 1.42M17.65 17.65l1.42 1.42M2.5 12h2M19.5 12h2M4.93 19.07l1.42-1.42M17.65 6.35l1.42-1.42"/>
                            </svg>
                            <svg class="theme-toggle-glyph theme-toggle-moon" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                                <path d="M20.5 15.1A8.5 8.5 0 0 1 8.9 3.5 8.5 8.5 0 1 0 20.5 15.1Z"/>
                            </svg>
                            <span class="theme-toggle-thumb"></span>
                        </span>
                        <span class="visually-hidden" data-theme-label>"Light theme active"</span>
                    </button>
                </div>
        </aside>
    }
}

#[component]
fn AppNavItem(
    href: &'static str,
    label: &'static str,
    mark: &'static str,
    #[prop(default = false)] exact: bool,
) -> impl IntoView {
    let location = use_location();
    let is_active = RwSignal::new({
        let pathname = location.pathname.get_untracked();
        if exact {
            pathname == href
        } else {
            pathname == href || pathname.starts_with(&format!("{href}/"))
        }
    });
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        let pathname = location.pathname.get();
        is_active.set(if exact {
            pathname == href
        } else {
            pathname == href || pathname.starts_with(&format!("{href}/"))
        });
    });
    view! {
        <a
            class="workspace-nav-item"
            class:is-active=move || reactive_bool(is_active)
            aria-current=move || reactive_bool(is_active).then_some("page")
            href=href
        >
            <span class="workspace-nav-mark" aria-hidden="true">{mark}</span>
            <strong>{label}</strong>
        </a>
    }
}

fn reactive_bool(signal: RwSignal<bool>) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        signal.get()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        signal.get_untracked()
    }
}

#[cfg(feature = "ssr")]
#[must_use]
pub(crate) fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en" data-print-manager-hydrating="">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <title>"Print Manager"</title>
                <link id="print-manager" rel="stylesheet" href="/style.css"/>
                <script>{browser_scripts::THEME_INIT}</script>
                <script>{browser_scripts::DETAIL_DIALOG}</script>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
            </head>
            <body>
                <App/>
                <script>{browser_scripts::UNSAVED_GUARD}</script>
                <script>{browser_scripts::PDF_ACTION}</script>
                <script>{browser_scripts::THEME}</script>
            </body>
        </html>
    }
}

#[cfg(feature = "hydrate")]
pub(crate) fn hydrate() {
    leptos::mount::hydrate_body(App);
    browser_scripts::emit_hydration_complete();
}
