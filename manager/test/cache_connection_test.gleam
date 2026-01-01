/// Tests for cache/connection module
/// Tests basic cache operations: get, set, delete, patterns
import cache/connection as cache_conn
import gleam/option.{None, Some}
import gleeunit/should
import test_cache

// =============================================================================
// Basic Operations Tests
// =============================================================================

pub fn get_nonexistent_key_test() {
  test_cache.with_cache(fn(cache) {
    let result = cache_conn.get(cache, "nonexistent_key")
    should.equal(result, Ok(None))
  })
}

pub fn set_and_get_test() {
  test_cache.with_cache(fn(cache) {
    let key = "test_key"
    let value = "test_value"

    // Set value
    let set_result = cache_conn.set(cache, key, value, 300)
    should.be_ok(set_result)

    // Get value
    let get_result = cache_conn.get(cache, key)
    should.equal(get_result, Ok(Some(value)))
  })
}

pub fn set_overwrites_existing_test() {
  test_cache.with_cache(fn(cache) {
    let key = "overwrite_test"

    // Set initial value
    let _ = cache_conn.set(cache, key, "initial", 300)

    // Overwrite
    let _ = cache_conn.set(cache, key, "updated", 300)

    // Verify new value
    let result = cache_conn.get(cache, key)
    should.equal(result, Ok(Some("updated")))
  })
}

pub fn delete_key_test() {
  test_cache.with_cache(fn(cache) {
    let key = "delete_test"

    // Set then delete
    let _ = cache_conn.set(cache, key, "value", 300)
    let delete_result = cache_conn.delete(cache, key)
    should.be_ok(delete_result)

    // Verify deleted
    let result = cache_conn.get(cache, key)
    should.equal(result, Ok(None))
  })
}

pub fn delete_nonexistent_key_test() {
  test_cache.with_cache(fn(cache) {
    // Should succeed even for nonexistent keys
    let result = cache_conn.delete(cache, "never_existed")
    should.be_ok(result)
  })
}

// =============================================================================
// Pattern Operations Tests
// =============================================================================

pub fn delete_pattern_test() {
  test_cache.with_cache(fn(cache) {
    // Set multiple keys with same prefix
    let _ = cache_conn.set(cache, "prefix:1", "value1", 300)
    let _ = cache_conn.set(cache, "prefix:2", "value2", 300)
    let _ = cache_conn.set(cache, "prefix:3", "value3", 300)
    let _ = cache_conn.set(cache, "other:1", "other_value", 300)

    // Delete pattern
    let delete_result = cache_conn.delete_pattern(cache, "prefix:*")
    should.be_ok(delete_result)

    // Verify prefix keys deleted
    should.equal(cache_conn.get(cache, "prefix:1"), Ok(None))
    should.equal(cache_conn.get(cache, "prefix:2"), Ok(None))
    should.equal(cache_conn.get(cache, "prefix:3"), Ok(None))

    // Verify other key remains
    should.equal(cache_conn.get(cache, "other:1"), Ok(Some("other_value")))
  })
}

pub fn delete_pattern_no_matches_test() {
  test_cache.with_cache(fn(cache) {
    // Should succeed even with no matches
    let result = cache_conn.delete_pattern(cache, "nonexistent:*")
    should.be_ok(result)
  })
}

// =============================================================================
// Key Generation Tests
// =============================================================================

pub fn entity_key_test() {
  let key = cache_conn.entity_key("chat", "123")
  should.equal(key, "chat:123")
}

pub fn list_key_test() {
  let key = cache_conn.list_key("projects")
  should.equal(key, "projects:list")
}

pub fn filtered_list_key_test() {
  let key = cache_conn.filtered_list_key("tasks", "status=pending")
  should.equal(key, "tasks:list:status=pending")
}

pub fn invalidation_pattern_test() {
  let pattern = cache_conn.invalidation_pattern("sources")
  should.equal(pattern, "sources:*")
}

// =============================================================================
// Cache-Aside Pattern Tests
// =============================================================================

pub fn get_or_compute_returns_cached_test() {
  test_cache.with_cache(fn(cache) {
    let key = "compute_test"

    // Pre-populate cache
    let _ = cache_conn.set(cache, key, "cached_value", 300)

    // get_or_compute should return cached value without calling compute
    let result =
      cache_conn.get_or_compute(cache, key, 300, fn() { Ok("computed_value") })

    should.equal(result, Ok("cached_value"))
  })
}

pub fn get_or_compute_computes_on_miss_test() {
  test_cache.with_cache(fn(cache) {
    let key = "compute_miss_test"

    // get_or_compute should call compute and cache result
    let result =
      cache_conn.get_or_compute(cache, key, 300, fn() { Ok("computed_value") })

    should.equal(result, Ok("computed_value"))

    // Verify it was cached
    let cached = cache_conn.get(cache, key)
    should.equal(cached, Ok(Some("computed_value")))
  })
}

pub fn get_or_compute_propagates_compute_error_test() {
  test_cache.with_cache(fn(cache) {
    let key = "compute_error_test"

    let result =
      cache_conn.get_or_compute(cache, key, 300, fn() {
        Error("compute failed")
      })

    should.equal(result, Error("compute failed"))

    // Verify nothing was cached
    let cached = cache_conn.get(cache, key)
    should.equal(cached, Ok(None))
  })
}
