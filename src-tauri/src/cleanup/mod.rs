//! Cleanup engine. Safety model (see PLAN.md §4):
//! analysis never mutates; execution re-validates every item, moves (never
//! copies, never deletes) into a same-volume holding area, records the original
//! path, and only after the retention period hands items to the Recycle Bin.

pub mod analyze;
pub mod blocklist;
pub mod duplicates;
pub mod holding;
pub mod rules;

use serde::{Deserialize, Serialize};

pub const CATEGORIES: &[(&str, &str)] = &[
    ("duplicates", "Duplicates"),
    ("large_cold", "Large and cold files"),
    ("temp", "Temp and caches"),
    ("junk", "App-generated junk"),
    ("installers", "Old installers"),
    ("empty_folders", "Empty folders"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub name: String,
    pub size: i64,
    pub category: String,
    pub reason: String,
    pub modified: Option<i64>,
    pub is_dir: bool,
    /// Duplicate group id (duplicates category only).
    pub group: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CategorySummary {
    pub id: String,
    pub label: String,
    pub count: usize,
    /// Bytes that cleaning would free. For duplicates: all copies but one.
    pub reclaimable: i64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CleanupSummary {
    pub analyzed_at: i64,
    pub total_reclaimable: i64,
    pub categories: Vec<CategorySummary>,
}

/// One item the user chose in the review list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanItem {
    pub path: String,
    pub category: String,
    /// Size seen at review time; the item is skipped if it changed since.
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemOutcome {
    pub path: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub batch_id: String,
    pub dry_run: bool,
    pub moved: usize,
    pub skipped: usize,
    pub bytes: i64,
    pub outcomes: Vec<ItemOutcome>,
}
