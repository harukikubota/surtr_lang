use std::path::PathBuf;

use surtr_analysis::{DocumentStore, Utf16Position};

#[test]
fn document_store_updates_text_version_hash_and_line_index() {
    let mut store = DocumentStore::default();
    let path = PathBuf::from("/repo/main.srt");

    let first = store.update_document(path.clone(), Some(1), "print(\"hi\")".to_string());
    let second = store.update_document(path.clone(), Some(2), "a\n😀b".to_string());

    assert_ne!(first.content_hash, second.content_hash);
    let document = store.get(&path).expect("document should be stored");
    assert_eq!(document.version, Some(2));
    assert_eq!(
        document.line_index.utf16_position_to_byte(Utf16Position {
            line: 1,
            character: 2
        }),
        Some(6)
    );
}

#[test]
fn document_store_lists_open_document_versions_in_path_order() {
    let mut store = DocumentStore::default();
    store.update_document(PathBuf::from("/repo/b.srt"), Some(2), "b".to_string());
    store.update_document(PathBuf::from("/repo/a.srt"), Some(1), "a".to_string());

    let versions = store.open_document_versions();

    assert_eq!(versions[0].path, PathBuf::from("/repo/a.srt"));
    assert_eq!(versions[0].version, Some(1));
    assert_eq!(versions[1].path, PathBuf::from("/repo/b.srt"));
    assert_eq!(versions[1].version, Some(2));
}

#[test]
fn document_store_removes_closed_documents() {
    let mut store = DocumentStore::default();
    let path = PathBuf::from("/repo/main.srt");
    store.update_document(path.clone(), Some(1), "main()".to_string());

    let removed = store.remove(&path).expect("document should be removed");

    assert_eq!(removed.text, "main()");
    assert!(store.get(&path).is_none());
}
