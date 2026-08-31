use desktop_pet::{app::Application, config::AppConfig};

#[test]
fn application_constructs_with_phase_one_window_spec() {
    let app = Application::new(AppConfig::default()).expect("default startup must succeed");

    let spec = app.window_spec();
    assert!(spec.transparent);
    assert!(spec.always_on_top);
    assert!(!spec.decorations);
    assert!(!spec.resizable);
}
