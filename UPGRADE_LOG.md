# Dependency Upgrade Log

**Date:** 2026-01-18  |  **Project:** caut  |  **Language:** Rust

## Summary
- **Updated:** 12  |  **Skipped:** 1  |  **Failed:** 0  |  **Needs attention:** 0

## Updates

### clap: 4.5 → 4.5.54
- **Breaking:** None
- **Notes:** Patch updates only

### clap_complete: 4.5 → 4.5.44
- **Breaking:** None
- **Notes:** Aligned with clap version

### serde: 1.0 → 1.0.228
- **Breaking:** None
- **Notes:** Patch updates only

### serde_json: 1.0 → 1.0.140
- **Breaking:** None
- **Notes:** Patch updates only

### anyhow: 1.0 → 1.0.100
- **Breaking:** None
- **Notes:** Patch updates only

### thiserror: 2.0 → 2.0.17
- **Breaking:** None
- **Notes:** Patch updates only

### tokio: 1 → 1.44
- **Breaking:** None
- **Notes:** Using LTS-compatible version (1.44.x)

### reqwest: 0.13 → 0.13.1
- **Breaking:** None
- **Notes:** Patch update

### chrono: 0.4 → 0.4.42
- **Breaking:** None
- **Notes:** Patch updates only

### keyring: 3.6 → 3.6.3
- **Breaking:** None
- **Notes:** Patch updates only

### crossterm: 0.29 → 0.29.0
- **Breaking:** None
- **Notes:** Already at latest 0.29.x

### vergen-gix: 9.1 → 9.1 (unchanged)
- **Breaking:** None
- **Notes:** Requires Rust 1.88+; rust-version updated from 1.85 to 1.88

## Skipped

### rich_rust: path dependency
- **Reason:** Local path dependency, not from crates.io
- **Action:** No change needed

## Notes

- All dependencies checked against crates.io as of 2026-01-18
- Using explicit patch versions for reproducibility
- Tokio version chosen for LTS compatibility
