//! Shared test factories, seeded datasets, and trait test suites.
//!
//! Every storage backend must pass the trait test suites defined here.
//! This ensures behavioral equivalence across `PostgresBackend`,
//! `HybridBackend`, and `KurrentBackend`.
//!
//! # Modules
//!
//! - [`factories`]: Functions to create test events, refs, links
//! - [`dataset`]: A seeded mini-dataset for deterministic assertions
//! - [`trait_tests`]: Exhaustive test suites that run against any backend

pub mod dataset;
pub mod factories;
pub mod trait_tests;
