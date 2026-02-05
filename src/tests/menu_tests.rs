use viberails::{MenuAction, get_menu_options};

#[test]
fn test_menu_options_count() {
    let options = get_menu_options();
    // Should have 8 menu options (user-facing only, not hidden callbacks)
    // Initialize Team, Join Team, Install Hooks, Remove Hooks, List Hooks,
    // Show Configuration, Uninstall Everything, Quit
    assert_eq!(options.len(), 8);
}

#[test]
fn test_menu_options_labels_are_unique() {
    let options = get_menu_options();
    let labels: Vec<_> = options.iter().map(|o| o.label).collect();

    let mut unique_labels = labels.clone();
    unique_labels.sort();
    unique_labels.dedup();
    assert_eq!(labels.len(), unique_labels.len());
}

#[test]
fn test_menu_options_labels_not_empty() {
    let options = get_menu_options();
    for option in &options {
        assert!(!option.label.is_empty());
    }
}

#[test]
fn test_menu_options_initialize_team_is_first() {
    let options = get_menu_options();
    // Initialize Team should be first (most common action for new users)
    assert_eq!(options[0].action, MenuAction::InitializeTeam);
}

#[test]
fn test_menu_options_contains_all_actions() {
    let options = get_menu_options();
    let actions: Vec<_> = options.iter().map(|o| o.action).collect();

    assert!(actions.contains(&MenuAction::InitializeTeam));
    assert!(actions.contains(&MenuAction::JoinTeam));
    assert!(actions.contains(&MenuAction::InstallHooks));
    assert!(actions.contains(&MenuAction::UninstallHooks));
    assert!(actions.contains(&MenuAction::UninstallAll));
    assert!(actions.contains(&MenuAction::ListHooks));
    assert!(actions.contains(&MenuAction::ShowConfiguration));
    assert!(actions.contains(&MenuAction::Quit));
}

#[test]
fn test_menu_lookup_finds_initialize_team() {
    let options = get_menu_options();
    let label = options
        .iter()
        .find(|o| o.action == MenuAction::InitializeTeam)
        .map(|o| o.label)
        .unwrap();

    let found = options
        .into_iter()
        .find(|o| o.label == label)
        .map(|o| o.action);

    assert_eq!(found, Some(MenuAction::InitializeTeam));
}

#[test]
fn test_menu_lookup_finds_join_team() {
    let options = get_menu_options();
    let label = options
        .iter()
        .find(|o| o.action == MenuAction::JoinTeam)
        .map(|o| o.label)
        .unwrap();

    let found = options
        .into_iter()
        .find(|o| o.label == label)
        .map(|o| o.action);

    assert_eq!(found, Some(MenuAction::JoinTeam));
}

#[test]
fn test_menu_lookup_unknown_label_returns_none() {
    let options = get_menu_options();

    let found = options
        .into_iter()
        .find(|o| o.label == "Unknown Action")
        .map(|o| o.action);

    assert_eq!(found, None);
}
