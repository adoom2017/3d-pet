use desktop_pet::{app::Application, config::AppConfig};

#[test]
fn application_constructs_and_exits_cleanly() {
    let app = Application::new(AppConfig::default()).expect("default startup must succeed");

    app.run();
}
