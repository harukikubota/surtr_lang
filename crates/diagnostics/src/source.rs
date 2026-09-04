use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub id: SourceId,
    pub file_name: String,
    pub source: String,
}

#[derive(Debug, Default, Clone)]
pub struct SourceRegistry {
    entries: Vec<SourceEntry>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        file_name: impl Into<String>,
        source: impl Into<String>,
    ) -> SourceId {
        let id = SourceId(self.entries.len() as u32);
        self.entries.push(SourceEntry {
            id,
            file_name: file_name.into(),
            source: source.into(),
        });
        id
    }

    pub fn get(&self, source_id: SourceId) -> Option<&SourceEntry> {
        self.entries.get(source_id.0 as usize)
    }

    pub fn file_name(&self, source_id: SourceId) -> Option<&str> {
        self.get(source_id).map(|entry| entry.file_name.as_str())
    }

    pub fn source(&self, source_id: SourceId) -> Option<&str> {
        self.get(source_id).map(|entry| entry.source.as_str())
    }

    pub fn update_source(&mut self, source_id: SourceId, source: impl Into<String>) -> bool {
        if let Some(entry) = self.entries.get_mut(source_id.0 as usize) {
            entry.source = source.into();
            true
        } else {
            false
        }
    }

    pub fn owned_context(&self, source_id: SourceId) -> Option<(String, String)> {
        self.get(source_id)
            .map(|entry| (entry.source.clone(), entry.file_name.clone()))
    }

    pub fn entries(&self) -> &[SourceEntry] {
        &self.entries
    }
}
