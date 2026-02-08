//! Auto-pagination for ServiceNow Table API responses.
//!
//! ServiceNow returns paginated results with:
//! - `X-Total-Count` header: total number of matching records
//! - `Link` header with `rel="next"`: URL for the next page
//!
//! The paginator fetches pages automatically, yielding records
//! as a stream until all records are retrieved or `--limit` is reached.

/// Configuration for pagination behavior.
pub struct PaginationConfig {
    /// Maximum total records to return. None means fetch all.
    pub limit: Option<usize>,

    /// Number of records per page (ServiceNow default: 100).
    pub page_size: usize,
}

impl Default for PaginationConfig {
    fn default() -> Self {
        Self {
            limit: None,
            page_size: 100,
        }
    }
}

impl PaginationConfig {
    pub fn with_limit(mut self, limit: Option<usize>) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PaginationConfig::default();
        assert_eq!(config.page_size, 100);
        assert!(config.limit.is_none());
    }

    #[test]
    fn test_with_limit() {
        let config = PaginationConfig::default().with_limit(Some(50));
        assert_eq!(config.limit, Some(50));
        assert_eq!(config.page_size, 100); // unchanged
    }

    #[test]
    fn test_with_limit_none() {
        let config = PaginationConfig::default()
            .with_limit(Some(50))
            .with_limit(None);
        assert!(config.limit.is_none());
    }

    #[test]
    fn test_with_page_size() {
        let config = PaginationConfig::default().with_page_size(25);
        assert_eq!(config.page_size, 25);
        assert!(config.limit.is_none()); // unchanged
    }

    #[test]
    fn test_builder_chaining() {
        let config = PaginationConfig::default()
            .with_page_size(50)
            .with_limit(Some(200));
        assert_eq!(config.page_size, 50);
        assert_eq!(config.limit, Some(200));
    }
}
