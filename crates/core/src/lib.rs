//! Parsing and reference-closure filtering engine for OpenAPI 3.0.x and 3.1.x documents.
//!
//! This crate has no `clap`, `std::fs`, or `std::io::stdin` dependencies: parsing operates on
//! document content already in memory (`&str`), and filtering operates on the resulting
//! `serde_json::Value` tree. Acquiring that content (reading a file, reading stdin, ...) is left
//! entirely to the caller — see the `openapiv3-filter` CLI crate for an example of a thin
//! file/stdin-acquisition layer built on top of this crate.
//!
//! # Example
//!
//! ```
//! use openapiv3_filter_core::filter::openapi::{FilteringParameters, OpenAPIFilter};
//! use openapiv3_filter_core::parser;
//!
//! let document = r#"{
//!     "openapi": "3.0.3",
//!     "info": { "title": "Example", "version": "1.0.0" },
//!     "paths": {
//!         "/pets": {
//!             "get": { "responses": { "200": { "description": "ok" } } }
//!         }
//!     }
//! }"#;
//!
//! let value = parser::parse(document).expect("valid OpenAPI document");
//! let filtered = value.filter_by_parameters(FilteringParameters {
//!     paths: Some(vec![String::from("/pets")]),
//!     ..Default::default()
//! });
//! println!("{}", serde_json::to_string_pretty(&filtered).unwrap());
//! ```

pub mod error;
pub mod filter;
pub mod parser;

pub use error::Error;
