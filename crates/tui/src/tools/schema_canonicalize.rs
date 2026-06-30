//! Byte-level canonicalization of JSON Schema for prefix-cache stability.
//!
//! When MCP servers return tool schemas, the field order within each schema
//! object and the order of entries in `required` / `dependentRequired` arrays
//! can vary across reconnections. This module normalizes those orderings so
//! that two logically equivalent schemas always produce identical bytes after
//! serialization.
//!
//! The approach mirrors `reasonix/internal/provider/schema_canonicalize.go`:
//!
//! 1. Sort every `"required"` array alphabetically.
//! 2. Sort every `"dependentRequired"` sub-array alphabetically.
//! 3. Recurse into all nested objects and arrays.
//!
//! `serde_json::Value::Object` uses `IndexMap` when `preserve_order` is
//! enabled (which this crate does). We therefore rebuild the map with sorted
//! keys to guarantee deterministic key ordering.

use serde_json::Value;

/// Recursively canonicalize a JSON Schema value in-place.
///
/// After canonicalization, two schemas that are semantically equivalent
/// (same keys, same `required` set, same `dependentRequired` sets) will
/// serialize to byte-identical JSON regardless of the original field or
/// array order.
pub fn canonicalize_schema(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // Sort `required` arrays (they are sets per JSON Schema spec).
            if let Some(Value::Array(req)) = map.get_mut("required") {
                sort_string_array(req);
            }
            // Sort `dependentRequired` sub-arrays.
            if let Some(Value::Object(deps)) = map.get_mut("dependentRequired") {
                for dep_value in deps.values_mut() {
                    if let Value::Array(arr) = dep_value {
                        sort_string_array(arr);
                    }
                }
            }
            // Recurse into every child value.
            for v in map.values_mut() {
                canonicalize_schema(v);
            }
            // Rebuild the map with sorted keys so serialization is deterministic.
            // serde_json::Map backed by IndexMap (preserve_order) doesn't have
            // drain(), so we swap to a temporary and rebuild.
            let old = std::mem::take(map);
            let mut entries: Vec<(String, Value)> = old.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in entries {
                map.insert(k, v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                canonicalize_schema(v);
            }
        }
        _ => {}
    }
}

/// Sort a JSON array of string values alphabetically in-place.
///
/// Non-string entries are left at the end in their original relative order.
fn sort_string_array(arr: &mut [Value]) {
    arr.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
}

#[cfg(test)]
mod tests {}
