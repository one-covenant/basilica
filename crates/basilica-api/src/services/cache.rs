//! Cache service with pluggable storage backends

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Cache storage trait for different backends
#[async_trait]
pub trait CacheStorage: Send + Sync {
    /// Get a value from cache
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError>;

    /// Set a value in cache with optional TTL
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>)
        -> Result<(), CacheError>;

    /// Delete a value from cache
    async fn delete(&self, key: &str) -> Result<(), CacheError>;

    /// Clear all values from cache
    async fn clear(&self) -> Result<(), CacheError>;

    /// Get cache statistics
    async fn stats(&self) -> Result<CacheStats, CacheError>;
}

/// Cache service with TTL and invalidation support
pub struct CacheService {
    storage: Box<dyn CacheStorage>,
    default_ttl: Option<Duration>,
}

/// Cache entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    value: Vec<u8>,
    expires_at: Option<u64>,
    created_at: u128, // Use u128 for nanosecond precision
}

/// Cache statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_size_bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

/// Cache errors
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid key: {0}")]
    InvalidKey(String),
}

/// In-memory cache storage
pub struct MemoryCacheStorage {
    data: Arc<RwLock<HashMap<String, CacheEntry>>>,
    stats: Arc<RwLock<CacheStats>>,
    max_size: Option<usize>,
}

/// File-based cache storage
pub struct FileCacheStorage {
    base_dir: PathBuf,
    stats: Arc<RwLock<CacheStats>>,
}

impl CacheService {
    /// Create a new cache service
    pub fn new(storage: Box<dyn CacheStorage>) -> Self {
        Self {
            storage,
            default_ttl: None,
        }
    }

    /// Create a cache service with default TTL
    pub fn with_ttl(storage: Box<dyn CacheStorage>, ttl: Duration) -> Self {
        Self {
            storage,
            default_ttl: Some(ttl),
        }
    }

    /// Get a value from cache (generic)
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, CacheError> {
        let data = self.storage.get(key).await?;
        match data {
            Some(bytes) => {
                let value = serde_json::from_slice(&bytes)
                    .map_err(|e| CacheError::Serialization(e.to_string()))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Set a value in cache (generic)
    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError> {
        let bytes =
            serde_json::to_vec(value).map_err(|e| CacheError::Serialization(e.to_string()))?;
        let ttl = ttl.or(self.default_ttl);
        self.storage.set(key, bytes, ttl).await
    }

    /// Delete a value from cache
    pub async fn delete(&self, key: &str) -> Result<(), CacheError> {
        self.storage.delete(key).await
    }

    /// Clear all cache entries
    pub async fn clear(&self) -> Result<(), CacheError> {
        self.storage.clear().await
    }

    /// Get cache statistics
    pub async fn stats(&self) -> Result<CacheStats, CacheError> {
        self.storage.stats().await
    }

    /// Invalidate entries matching a pattern
    pub async fn invalidate_pattern(&self, _pattern: &str) -> Result<u32, CacheError> {
        // This is a simple implementation - could be optimized per backend
        let count = 0;
        // Note: This would need to be implemented properly with key listing
        // For now, returning 0 as placeholder
        Ok(count)
    }
}

impl Default for MemoryCacheStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryCacheStorage {
    /// Create a new memory cache storage
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(CacheStats::default())),
            max_size: None,
        }
    }

    /// Create memory cache with size limit
    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(CacheStats::default())),
            max_size: Some(max_size),
        }
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn current_timestamp_nanos() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    async fn cleanup_expired(&self) {
        let now = Self::current_timestamp();
        let mut data = self.data.write().await;
        let mut stats = self.stats.write().await;

        let expired_keys: Vec<String> = data
            .iter()
            .filter(|(_, entry)| entry.expires_at.is_some_and(|exp| exp <= now))
            .map(|(k, _)| k.clone())
            .collect();

        for key in expired_keys {
            data.remove(&key);
            stats.evictions += 1;
        }
    }
}

#[async_trait]
impl CacheStorage for MemoryCacheStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        self.cleanup_expired().await;

        let data = self.data.read().await;
        let mut stats = self.stats.write().await;

        match data.get(key) {
            Some(entry) => {
                let now = Self::current_timestamp();
                if let Some(expires_at) = entry.expires_at {
                    if expires_at <= now {
                        stats.misses += 1;
                        return Ok(None);
                    }
                }
                stats.hits += 1;
                Ok(Some(entry.value.clone()))
            }
            None => {
                stats.misses += 1;
                Ok(None)
            }
        }
    }

    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError> {
        let entry = CacheEntry {
            value: value.clone(),
            expires_at: ttl.map(|d| Self::current_timestamp() + d.as_secs()),
            created_at: Self::current_timestamp_nanos(),
        };

        let mut data = self.data.write().await;
        let mut stats = self.stats.write().await;

        // Check size limit
        if let Some(max_size) = self.max_size {
            if data.len() >= max_size && !data.contains_key(key) {
                // Simple eviction: remove oldest entry
                if let Some(oldest_key) = data
                    .iter()
                    .min_by_key(|(_, e)| e.created_at)
                    .map(|(k, _)| k.clone())
                {
                    data.remove(&oldest_key);
                    stats.evictions += 1;
                }
            }
        }

        data.insert(key.to_string(), entry);
        stats.total_entries = data.len();
        stats.total_size_bytes = data.values().map(|e| e.value.len()).sum();

        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let mut data = self.data.write().await;
        let mut stats = self.stats.write().await;

        data.remove(key);
        stats.total_entries = data.len();
        stats.total_size_bytes = data.values().map(|e| e.value.len()).sum();

        Ok(())
    }

    async fn clear(&self) -> Result<(), CacheError> {
        let mut data = self.data.write().await;
        let mut stats = self.stats.write().await;

        data.clear();
        stats.total_entries = 0;
        stats.total_size_bytes = 0;

        Ok(())
    }

    async fn stats(&self) -> Result<CacheStats, CacheError> {
        Ok(self.stats.read().await.clone())
    }
}

impl FileCacheStorage {
    /// Create a new file cache storage
    pub fn new(base_dir: impl AsRef<Path>) -> Result<Self, CacheError> {
        let base_dir = base_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_dir)?;

        Ok(Self {
            base_dir,
            stats: Arc::new(RwLock::new(CacheStats::default())),
        })
    }

    fn key_to_path(&self, key: &str) -> Result<PathBuf, CacheError> {
        // Simple validation to prevent directory traversal
        if key.contains("..") || key.contains('/') || key.contains('\\') {
            return Err(CacheError::InvalidKey(format!("Invalid key: {}", key)));
        }

        Ok(self.base_dir.join(format!("{}.cache", key)))
    }
}

#[async_trait]
impl CacheStorage for FileCacheStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, CacheError> {
        let path = self.key_to_path(key)?;
        let mut stats = self.stats.write().await;

        if !path.exists() {
            stats.misses += 1;
            return Ok(None);
        }

        let data = tokio::fs::read(&path).await?;
        let entry: CacheEntry =
            serde_json::from_slice(&data).map_err(|e| CacheError::Serialization(e.to_string()))?;

        // Check expiration
        if let Some(expires_at) = entry.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            if expires_at <= now {
                // Expired, remove file
                tokio::fs::remove_file(&path).await?;
                stats.misses += 1;
                stats.evictions += 1;
                return Ok(None);
            }
        }

        stats.hits += 1;
        Ok(Some(entry.value))
    }

    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError> {
        let path = self.key_to_path(key)?;

        let entry = CacheEntry {
            value,
            expires_at: ttl.map(|d| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + d.as_secs()
            }),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        };

        let data =
            serde_json::to_vec(&entry).map_err(|e| CacheError::Serialization(e.to_string()))?;

        tokio::fs::write(&path, data).await?;

        // Update stats
        let mut stats = self.stats.write().await;
        let mut entries = tokio::fs::read_dir(&self.base_dir).await?;
        let mut count = 0;
        while let Some(_entry) = entries.next_entry().await? {
            count += 1;
        }
        stats.total_entries = count;

        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let path = self.key_to_path(key)?;

        if path.exists() {
            tokio::fs::remove_file(&path).await?;
        }

        Ok(())
    }

    async fn clear(&self) -> Result<(), CacheError> {
        let mut entries = tokio::fs::read_dir(&self.base_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("cache") {
                tokio::fs::remove_file(entry.path()).await?;
            }
        }

        let mut stats = self.stats.write().await;
        stats.total_entries = 0;
        stats.total_size_bytes = 0;

        Ok(())
    }

    async fn stats(&self) -> Result<CacheStats, CacheError> {
        Ok(self.stats.read().await.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_memory_cache_storage_set_get() {
        let storage = MemoryCacheStorage::new();
        let cache = CacheService::new(Box::new(storage));

        // Test set and get
        cache.set("key1", &"value1", None).await.unwrap();
        let value: Option<String> = cache.get("key1").await.unwrap();
        assert_eq!(value, Some("value1".to_string()));
    }

    #[tokio::test]
    async fn test_memory_cache_storage_expiry() {
        let storage = MemoryCacheStorage::new();
        let cache = CacheService::new(Box::new(storage));

        // Set with very short TTL
        cache
            .set("key1", &"value1", Some(Duration::from_millis(1)))
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        let value: Option<String> = cache.get("key1").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_file_cache_storage_persistence() {
        let temp_dir = std::env::temp_dir().join("test_cache");
        let _ = std::fs::remove_dir_all(&temp_dir); // Clean up if exists

        {
            let storage = FileCacheStorage::new(&temp_dir).unwrap();
            let cache = CacheService::new(Box::new(storage));

            cache
                .set("persist_key", &"persist_value", None)
                .await
                .unwrap();
        }

        // Create new instance and check if data persists
        {
            let storage = FileCacheStorage::new(&temp_dir).unwrap();
            let cache = CacheService::new(Box::new(storage));

            let value: Option<String> = cache.get("persist_key").await.unwrap();
            assert_eq!(value, Some("persist_value".to_string()));
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_file_cache_storage_concurrent_access() {
        let temp_dir = std::env::temp_dir().join("test_cache_concurrent");
        let _ = std::fs::remove_dir_all(&temp_dir);

        let storage = FileCacheStorage::new(&temp_dir).unwrap();
        let cache = Arc::new(CacheService::new(Box::new(storage)));

        let mut handles = vec![];

        // Spawn multiple tasks writing to cache
        for i in 0..10 {
            let cache_clone = cache.clone();
            let handle = tokio::spawn(async move {
                cache_clone
                    .set(&format!("key{}", i), &format!("value{}", i), None)
                    .await
                    .unwrap();
            });
            handles.push(handle);
        }

        // Wait for all writes to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all values
        for i in 0..10 {
            let value: Option<String> = cache.get(&format!("key{}", i)).await.unwrap();
            assert_eq!(value, Some(format!("value{}", i)));
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_cache_service_ttl() {
        let storage = MemoryCacheStorage::new();
        let cache = CacheService::with_ttl(Box::new(storage), Duration::from_secs(1));

        // Set without explicit TTL (should use default)
        cache.set("ttl_key", &"ttl_value", None).await.unwrap();

        // Value should exist immediately
        let value: Option<String> = cache.get("ttl_key").await.unwrap();
        assert_eq!(value, Some("ttl_value".to_string()));

        // Wait for default TTL to expire
        tokio::time::sleep(Duration::from_secs(2)).await;

        let value: Option<String> = cache.get("ttl_key").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_cache_service_invalidation() {
        let storage = MemoryCacheStorage::new();
        let cache = CacheService::new(Box::new(storage));

        cache.set("key1", &"value1", None).await.unwrap();
        cache.set("key2", &"value2", None).await.unwrap();

        // Delete specific key
        cache.delete("key1").await.unwrap();

        let value1: Option<String> = cache.get("key1").await.unwrap();
        let value2: Option<String> = cache.get("key2").await.unwrap();

        assert_eq!(value1, None);
        assert_eq!(value2, Some("value2".to_string()));

        // Clear all
        cache.clear().await.unwrap();

        let value2: Option<String> = cache.get("key2").await.unwrap();
        assert_eq!(value2, None);
    }

    #[tokio::test]
    async fn test_cache_service_stats() {
        let storage = MemoryCacheStorage::new();
        let cache_storage = Box::new(storage);

        // Set some values
        cache_storage
            .set("key1", vec![1, 2, 3], None)
            .await
            .unwrap();
        cache_storage
            .set("key2", vec![4, 5, 6], None)
            .await
            .unwrap();

        // Get values to update hit/miss stats
        let _ = cache_storage.get("key1").await.unwrap();
        let _ = cache_storage.get("nonexistent").await.unwrap();

        let stats = cache_storage.stats().await.unwrap();
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn test_memory_cache_with_max_size() {
        let storage = MemoryCacheStorage::with_max_size(2);
        let cache = CacheService::new(Box::new(storage));

        // Fill cache to max size with small delays to ensure different timestamps
        cache.set("key1", &"value1", None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;

        cache.set("key2", &"value2", None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Adding third item should evict oldest
        cache.set("key3", &"value3", None).await.unwrap();

        // key1 should be evicted (oldest)
        let value1: Option<String> = cache.get("key1").await.unwrap();
        let value2: Option<String> = cache.get("key2").await.unwrap();
        let value3: Option<String> = cache.get("key3").await.unwrap();

        assert_eq!(value1, None);
        assert_eq!(value2, Some("value2".to_string()));
        assert_eq!(value3, Some("value3".to_string()));
    }
}
