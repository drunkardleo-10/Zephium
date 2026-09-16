#![forbid(unsafe_code)]

pub mod blocker;
pub mod commands;
pub mod extensions;
pub mod geometry;
pub mod icon;
pub mod ids;
pub mod injection;
pub mod item;
pub mod items;
pub mod layout;
pub mod macos;
pub mod navigation;
pub mod permissions;
pub mod ports;
pub mod profiles;
pub mod runtime_security;
pub mod search;
pub mod session;
pub mod spaces;
pub mod split;
pub mod userscripts;
pub mod webkitgtk;
pub mod webview2;
pub mod windows;

/// Allowlisted application appearance preferences.
pub mod preferences;

/// Durable user-authored resources shared by Browse and Work.
pub mod resources;
