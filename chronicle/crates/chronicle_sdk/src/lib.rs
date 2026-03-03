//! Chronicle Rust SDK.
//!
//! A client library for interacting with a Chronicle server over HTTP.
//!
//! # Quick Start
//!
//! ```no_run
//! use chronicle_sdk::{ChronicleClient, ClientConfig};
//!
//! # async fn example() -> Result<(), chronicle_sdk::SdkError> {
//! let client = ChronicleClient::new(ClientConfig {
//!     endpoint: "http://localhost:3000".to_string(),
//!     org_id: "org_123".to_string(),
//!     api_key: Some("key_xxx".to_string()),
//! });
//!
//! // Log an event
//! client.log("stripe", "payments", "payment_intent.succeeded")
//!     .entity("customer", "cust_123")
//!     .payload(serde_json::json!({"amount": 4999}))
//!     .send()
//!     .await?;
//! # Ok(())
//! # }
//! ```

pub mod client;

pub use client::{ChronicleClient, ClientConfig, EventBuilder, SdkError};
