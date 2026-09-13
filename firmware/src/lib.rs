//! Host-testable pieces of the Pico client (URL, settings, HTTP, panel map).

#![cfg_attr(not(test), no_std)]

pub mod config;
pub mod headers;
pub mod panel;
pub mod protocol;
