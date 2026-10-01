#![allow(clippy::must_use_candidate)]

mod app;
#[cfg(feature = "ssr")]
mod app_services;
#[cfg(feature = "ssr")]
mod application;
#[cfg(any(feature = "ssr", feature = "hydrate", test))]
mod browser_scripts;

#[cfg(any(feature = "ssr", test))]
mod calculation;
#[cfg(feature = "ssr")]
mod config;
#[cfg(feature = "ssr")]
mod database;
#[cfg(feature = "ssr")]
mod domain;
#[cfg(feature = "ssr")]
mod email;
#[cfg(feature = "ssr")]
mod pdf;
mod product_types;
mod read_models;
#[cfg(feature = "ssr")]
pub(crate) mod repository;
#[cfg(feature = "ssr")]
mod storage;
mod ui;
#[cfg(feature = "ssr")]
mod web;

#[cfg(feature = "ssr")]
pub use application::run;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    app::hydrate();
}
