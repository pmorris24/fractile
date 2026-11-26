//! The version this build reports: in the Settings version label, the journal and the status
//! JSON.

/// Cargo's package version: every build is a source build.
pub fn build_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
