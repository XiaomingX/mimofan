//! Small in-process cache for deterministic non-streaming chat responses.

use std::num::NonZeroUsize;
use std::sync::{Mutex, OnceLock};

use lru::LruCache;
use sha2::{Digest, Sha256};

use crate::models::{MessageRequest, MessageResponse, Usage};

const DEFAULT_CAPACITY: usize = 256;

static RESPONSE_CACHE: OnceLock<ResponseCache> = OnceLock::new();

pub(crate) fn response_cache() -> &'static ResponseCache {
    RESPONSE_CACHE.get_or_init(ResponseCache::new)
}

pub(crate) fn request_is_cacheable(request: &MessageRequest) -> bool {
    request.stream != Some(true)
        && request.tools.as_ref().is_none_or(Vec::is_empty)
        && request.tool_choice.is_none()
        && request.temperature == Some(0.0)
        && request.top_p.is_none_or(|top_p| top_p == 1.0)
}

pub(crate) struct ResponseCache {
    inner: Mutex<LruCache<[u8; 32], MessageResponse>>,
}

impl ResponseCache {
    fn new() -> Self {
        Self::with_capacity(NonZeroUsize::new(DEFAULT_CAPACITY).expect("non-zero capacity"))
    }

    fn with_capacity(capacity: NonZeroUsize) -> Self {
        Self {
            inner: Mutex::new(LruCache::new(capacity)),
        }
    }

    pub(crate) fn make_key(
        provider: &str,
        base_url: &str,
        path_suffix: Option<&str>,
        api_key: &str,
        wire_body: &[u8],
    ) -> [u8; 32] {
        let mut hasher = Sha256::new();
        update_field(&mut hasher, provider.as_bytes());
        update_field(&mut hasher, base_url.as_bytes());
        update_field(&mut hasher, path_suffix.unwrap_or("").as_bytes());
        update_field(&mut hasher, &Sha256::digest(api_key.as_bytes()));
        update_field(&mut hasher, wire_body);
        hasher.finalize().into()
    }

    pub(crate) fn get(&self, key: &[u8; 32]) -> Option<MessageResponse> {
        let mut cache = self.inner.lock().ok()?;
        cache.get(key).cloned().map(|mut response| {
            response.usage = Usage::default();
            response
        })
    }

    pub(crate) fn put(&self, key: [u8; 32], value: MessageResponse) {
        if let Ok(mut cache) = self.inner.lock() {
            cache.put(key, value);
        }
    }
}

fn update_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {}
