//! One module per screen; the root swaps between them.

pub mod admin;
pub mod boot;
pub mod connection;
pub mod login;
pub mod sale;
pub mod setup;

pub use boot::Boot;
pub use connection::Connection;
pub use login::Login;
pub use setup::Setup;
