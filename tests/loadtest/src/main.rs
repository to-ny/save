use goose::prelude::*;
use save_loadtest::config::LoadTestConfig;
use save_loadtest::reporting::TestReport;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config_path = std::env::var("LOADTEST_CONFIG")
        .unwrap_or_else(|_| "tests/loadtest/config.toml".to_string());
    let scenario_name = std::env::var("LOADTEST_SCENARIO").unwrap_or_else(|_| "mixed".to_string());
    let report_name = std::env::var("LOADTEST_REPORT_NAME").ok();

    println!("🚀 Save Load Test");
    println!("   Config: {}", config_path);
    println!("   Scenario: {}", scenario_name);
    println!();

    let config = LoadTestConfig::from_file(&config_path)?;

    let scenario = match scenario_name.to_lowercase().as_str() {
        "read-heavy" | "read_heavy" => {
            save_loadtest::scenarios::read_heavy::build_scenario(&config)
        }
        "write-heavy" | "write_heavy" => {
            save_loadtest::scenarios::write_heavy::build_scenario(&config)
        }
        "mixed" => save_loadtest::scenarios::mixed::build_scenario(&config),
        "multipart" => save_loadtest::scenarios::multipart::build_scenario(&config),
        _ => {
            eprintln!("❌ Unknown scenario: {}", scenario_name);
            eprintln!("   Valid options: read-heavy, write-heavy, mixed, multipart");
            std::process::exit(1);
        }
    };

    let app_state = std::sync::Arc::new(save_loadtest::transactions::AppState::new(config.clone()));
    save_loadtest::GLOBAL_STATE.set(app_state).ok();

    let metrics = GooseAttack::initialize()?
        .register_scenario(scenario)
        .execute()
        .await?;

    let final_report_name = report_name.unwrap_or_else(|| scenario_name.clone());
    let report = TestReport::from_goose_metrics(&final_report_name, &metrics, vec![], vec![]);

    std::fs::create_dir_all(&config.reporting.output_dir)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let json_path = format!(
        "{}/{}-{}.json",
        config.reporting.output_dir, final_report_name, timestamp
    );
    report.save_json(&json_path)?;
    println!("\n📊 Report saved to: {}", json_path);

    let md_path = format!(
        "{}/{}-{}.md",
        config.reporting.output_dir, final_report_name, timestamp
    );
    report.save_markdown(&md_path)?;
    println!("📊 Report saved to: {}", md_path);

    Ok(())
}
