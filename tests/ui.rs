use armatra::{cache::ByteLru, ui::state::Preferences};
use std::sync::Arc;

#[test]
fn metadata_inspection_does_not_promote_cache_entries() {
    let mut cache = ByteLru::new(2);
    cache.insert("first", Arc::new(1), 1);
    cache.insert("second", Arc::new(2), 1);
    assert_eq!(cache.peek(&"first"), Some(&1));
    cache.insert("third", Arc::new(3), 1);
    assert!(!cache.contains(&"first"));
    assert!(cache.contains(&"second"));
}

#[test]
fn preferences_restore_and_recover_from_invalid_files() {
    let root = std::env::temp_dir().join(format!("armatra-ui-preferences-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let p = Preferences {
        assets_width: 310.,
        inspector_width: 290.,
        timeline_height: 165.,
        skeleton: true,
        ..Default::default()
    };
    p.save(&root).unwrap();
    let restored = Preferences::load(&root);
    assert_eq!(restored.assets_width, 310.);
    assert_eq!(restored.inspector_width, 290.);
    assert_eq!(restored.timeline_height, 165.);
    assert!(restored.skeleton);
    std::fs::write(
        root.join("ui-preferences.json"),
        br#"{"assets_width":99999,"timeline_height":-10,"scale":50}"#,
    )
    .unwrap();
    let clamped = Preferences::load(&root);
    assert_eq!(clamped.assets_width, 400.);
    assert_eq!(clamped.timeline_height, 130.);
    assert_eq!(clamped.scale, 1.5);
    std::fs::write(root.join("ui-preferences.json"), b"broken").unwrap();
    assert_eq!(
        Preferences::load(&root).assets_width,
        Preferences::default().assets_width
    );
    std::fs::remove_file(root.join("ui-preferences.json")).unwrap();
    std::fs::remove_dir(root).unwrap();
}
