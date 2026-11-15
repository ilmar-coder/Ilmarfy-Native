//! Núcleo compartilhado dos apps nativos do Ilmarfy (Android, Mac e Fedora):
//! cliente da API, cache local em SQLite, sincronização, fila de reprodução,
//! plays offline e downloads para ouvir sem internet.

pub mod api;
pub mod client;
pub mod error;
pub mod library;
pub mod models;
pub mod queue;

pub use api::{Api, DEFAULT_SERVER, Fetched};
pub use client::{Ilmarfy, Source, SyncReport};
pub use error::{Error, Result};
pub use library::{Library, TrackOrder};
pub use models::*;
pub use queue::{PlayQueue, Repeat};
