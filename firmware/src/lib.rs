//! Host-testable pieces of the Pico client (URL shaping, server parse).

#![cfg_attr(not(test), no_std)]

pub mod protocol;
