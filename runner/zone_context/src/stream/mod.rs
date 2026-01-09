//! Real-time streaming events
//!
//! Provides event types for streaming context gathering progress to clients.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Events emitted during context gathering
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GatheringEvent {
    /// Context gathering started
    Started {
        gathering_id: Uuid,
        source_count: usize,
        timestamp: DateTime<Utc>,
    },

    /// Started fetching from a source
    SourceStarted {
        gathering_id: Uuid,
        source_id: Uuid,
        source_name: String,
        source_type: String,
    },

    /// Progress on fetching from a source
    SourceProgress {
        gathering_id: Uuid,
        source_id: Uuid,
        items_fetched: usize,
        estimated_total: Option<usize>,
        tokens_fetched: usize,
    },

    /// Finished fetching from a source
    SourceCompleted {
        gathering_id: Uuid,
        source_id: Uuid,
        items_count: usize,
        token_count: usize,
        duration_ms: u64,
    },

    /// Error fetching from a source
    SourceError {
        gathering_id: Uuid,
        source_id: Uuid,
        error: String,
    },

    /// Started analyzing content
    AnalysisStarted {
        gathering_id: Uuid,
        total_items: usize,
    },

    /// Progress on analysis
    AnalysisProgress {
        gathering_id: Uuid,
        analyzed_count: usize,
        total_count: usize,
        current_stage: AnalysisStage,
    },

    /// Embedding generation progress
    EmbeddingProgress {
        gathering_id: Uuid,
        embedded_count: usize,
        total_count: usize,
    },

    /// Context gathering completed
    Completed {
        gathering_id: Uuid,
        total_items: usize,
        total_tokens: usize,
        duration_ms: u64,
        timestamp: DateTime<Utc>,
    },

    /// Context gathering failed
    Failed {
        gathering_id: Uuid,
        error: String,
        timestamp: DateTime<Utc>,
    },
}

impl GatheringEvent {
    /// Get the gathering ID for this event
    pub fn gathering_id(&self) -> Uuid {
        match self {
            Self::Started { gathering_id, .. }
            | Self::SourceStarted { gathering_id, .. }
            | Self::SourceProgress { gathering_id, .. }
            | Self::SourceCompleted { gathering_id, .. }
            | Self::SourceError { gathering_id, .. }
            | Self::AnalysisStarted { gathering_id, .. }
            | Self::AnalysisProgress { gathering_id, .. }
            | Self::EmbeddingProgress { gathering_id, .. }
            | Self::Completed { gathering_id, .. }
            | Self::Failed { gathering_id, .. } => *gathering_id,
        }
    }

    /// Check if this is a terminal event
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. })
    }
}

/// Analysis pipeline stages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStage {
    /// Generating embeddings
    Embedding,
    /// Extracting entities
    EntityExtraction,
    /// Categorizing content
    Categorization,
    /// Computing quality metrics
    Quality,
    /// Computing relevance scores
    Relevance,
}

/// Event broadcaster trait for streaming events
pub trait GatheringCallback: Send + Sync {
    /// Called when an event occurs
    fn on_event(&self, event: GatheringEvent);
}

/// No-op callback implementation
pub struct NoOpCallback;

impl GatheringCallback for NoOpCallback {
    fn on_event(&self, _event: GatheringEvent) {}
}

/// Callback that collects events into a Vec
pub struct CollectingCallback {
    events: std::sync::Mutex<Vec<GatheringEvent>>,
}

impl CollectingCallback {
    /// Create a new collecting callback
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Get collected events
    pub fn events(&self) -> Vec<GatheringEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl Default for CollectingCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl GatheringCallback for CollectingCallback {
    fn on_event(&self, event: GatheringEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gathering_event_started() {
        let id = Uuid::new_v4();
        let event = GatheringEvent::Started {
            gathering_id: id,
            source_count: 5,
            timestamp: Utc::now(),
        };

        assert_eq!(event.gathering_id(), id);
        assert!(!event.is_terminal());
    }

    #[test]
    fn test_gathering_event_completed_is_terminal() {
        let event = GatheringEvent::Completed {
            gathering_id: Uuid::new_v4(),
            total_items: 100,
            total_tokens: 50000,
            duration_ms: 5000,
            timestamp: Utc::now(),
        };

        assert!(event.is_terminal());
    }

    #[test]
    fn test_gathering_event_failed_is_terminal() {
        let event = GatheringEvent::Failed {
            gathering_id: Uuid::new_v4(),
            error: "Connection failed".to_string(),
            timestamp: Utc::now(),
        };

        assert!(event.is_terminal());
    }

    #[test]
    fn test_no_op_callback() {
        let callback = NoOpCallback;
        callback.on_event(GatheringEvent::Started {
            gathering_id: Uuid::new_v4(),
            source_count: 1,
            timestamp: Utc::now(),
        });
        // Should not panic
    }

    #[test]
    fn test_collecting_callback() {
        let callback = CollectingCallback::new();

        callback.on_event(GatheringEvent::Started {
            gathering_id: Uuid::new_v4(),
            source_count: 1,
            timestamp: Utc::now(),
        });

        let events = callback.events();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_event_serialization() {
        let event = GatheringEvent::SourceProgress {
            gathering_id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            items_fetched: 10,
            estimated_total: Some(100),
            tokens_fetched: 5000,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("source_progress"));
        assert!(json.contains("items_fetched"));
    }

    #[test]
    fn test_analysis_stage_serialization() {
        let stage = AnalysisStage::EntityExtraction;
        let json = serde_json::to_string(&stage).unwrap();
        assert_eq!(json, "\"entity_extraction\"");
    }
}
