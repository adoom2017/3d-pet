use desktop_pet::config::AppConfig;

fn main() {
    if let Err(error) = desktop_pet::run(AppConfig::default()) {
        eprintln!("DesktopPet failed: {error:#}");
        std::process::exit(1);
    }
}
