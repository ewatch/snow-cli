use super::*;
use tempfile::TempDir;

/// Helper: create a temp dir and return the config file path within it.
fn temp_config_path() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join(".servicenow").join("config.toml");
    (tmp, config_path)
}

#[tokio::test]
async fn test_config_init_creates_config() {
    let (_tmp, config_path) = temp_config_path();
    let result = handle_init(
        &config_path,
        Some("https://test.service-now.com".to_string()),
        Some(CliAuthMethod::Basic),
        Some("admin".to_string()),
        None,
        "default".to_string(),
    )
    .await;
    assert!(result.is_ok());

    let config = AppConfig::load_from(&config_path).unwrap();
    assert_eq!(config.default_profile, "default");
    let profile = config.profiles.get("default").unwrap();
    assert_eq!(profile.instance, "https://test.service-now.com");
    assert_eq!(profile.auth_method, AuthMethod::Basic);
    assert_eq!(profile.username, Some("admin".to_string()));
}

#[tokio::test]
async fn test_config_init_defaults_auth_to_basic() {
    let (_tmp, config_path) = temp_config_path();
    handle_init(
        &config_path,
        Some("https://test.service-now.com".to_string()),
        None,
        None,
        None,
        "default".to_string(),
    )
    .await
    .unwrap();

    let config = AppConfig::load_from(&config_path).unwrap();
    let profile = config.profiles.get("default").unwrap();
    assert_eq!(profile.auth_method, AuthMethod::Basic);
}

#[tokio::test]
async fn test_config_init_fails_without_instance() {
    let (_tmp, config_path) = temp_config_path();
    let result = handle_init(&config_path, None, None, None, None, "default".to_string()).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Instance URL is required")
    );
}

#[tokio::test]
async fn test_config_init_fails_if_exists() {
    let (_tmp, config_path) = temp_config_path();
    // Create config first
    handle_init(
        &config_path,
        Some("https://test.service-now.com".to_string()),
        None,
        None,
        None,
        "default".to_string(),
    )
    .await
    .unwrap();

    // Try init again — should fail
    let result = handle_init(
        &config_path,
        Some("https://test2.service-now.com".to_string()),
        None,
        None,
        None,
        "default".to_string(),
    )
    .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already exists"));
}

#[tokio::test]
async fn test_config_init_with_oauth_grant_type() {
    let (_tmp, config_path) = temp_config_path();
    handle_init(
        &config_path,
        Some("https://test.service-now.com".to_string()),
        Some(CliAuthMethod::Oauth2),
        Some("admin".to_string()),
        Some(CliOAuthGrantType::Password),
        "default".to_string(),
    )
    .await
    .unwrap();

    let config = AppConfig::load_from(&config_path).unwrap();
    let profile = config.profiles.get("default").unwrap();
    assert_eq!(profile.auth_method, AuthMethod::Oauth2);
    assert_eq!(profile.oauth_grant_type, Some(OAuthGrantType::Password));
}

#[tokio::test]
async fn test_config_set_profile_creates_new() {
    let (_tmp, config_path) = temp_config_path();
    // Init first
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        None,
        None,
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    // Create a second profile
    handle_set_profile(
        &config_path,
        "prod".to_string(),
        Some("https://prod.service-now.com".to_string()),
        Some(CliAuthMethod::Oauth2),
        None,
        Some("client123".to_string()),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let config = AppConfig::load_from(&config_path).unwrap();
    assert_eq!(config.profiles.len(), 2);
    let prod = config.profiles.get("prod").unwrap();
    assert_eq!(prod.instance, "https://prod.service-now.com");
    assert_eq!(prod.auth_method, AuthMethod::Oauth2);
    assert_eq!(prod.client_id, Some("client123".to_string()));
}

#[tokio::test]
async fn test_config_set_profile_updates_existing() {
    let (_tmp, config_path) = temp_config_path();
    // Init
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        Some(CliAuthMethod::Basic),
        Some("admin".to_string()),
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    // Update instance URL only — should preserve other fields
    handle_set_profile(
        &config_path,
        "dev".to_string(),
        Some("https://dev2.service-now.com".to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let config = AppConfig::load_from(&config_path).unwrap();
    let dev = config.profiles.get("dev").unwrap();
    assert_eq!(dev.instance, "https://dev2.service-now.com");
    // Auth method and username should be preserved
    assert_eq!(dev.auth_method, AuthMethod::Basic);
    assert_eq!(dev.username, Some("admin".to_string()));
}

#[tokio::test]
async fn test_config_set_profile_requires_instance_for_new() {
    let (_tmp, config_path) = temp_config_path();
    // Init
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        None,
        None,
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    // Try to create new profile without instance
    let result = handle_set_profile(
        &config_path,
        "prod".to_string(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Instance URL is required")
    );
}

#[tokio::test]
async fn test_config_set_profile_with_oauth_grant_type() {
    let (_tmp, config_path) = temp_config_path();
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        None,
        None,
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    handle_set_profile(
        &config_path,
        "oauth-prod".to_string(),
        Some("https://prod.service-now.com".to_string()),
        Some(CliAuthMethod::Oauth2),
        Some("svc_account".to_string()),
        Some("client_abc".to_string()),
        Some(CliOAuthGrantType::Password),
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let config = AppConfig::load_from(&config_path).unwrap();
    let profile = config.profiles.get("oauth-prod").unwrap();
    assert_eq!(profile.auth_method, AuthMethod::Oauth2);
    assert_eq!(profile.oauth_grant_type, Some(OAuthGrantType::Password));
    assert_eq!(profile.username, Some("svc_account".to_string()));
    assert_eq!(profile.client_id, Some("client_abc".to_string()));
}

#[tokio::test]
async fn test_config_use_profile() {
    let (_tmp, config_path) = temp_config_path();
    // Init with dev
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        None,
        None,
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    // Add prod
    handle_set_profile(
        &config_path,
        "prod".to_string(),
        Some("https://prod.service-now.com".to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    // Switch to prod
    handle_default_profile(&config_path, "prod".to_string())
        .await
        .unwrap();

    let config = AppConfig::load_from(&config_path).unwrap();
    assert_eq!(config.default_profile, "prod");
}

#[tokio::test]
async fn test_config_use_profile_fails_for_nonexistent() {
    let (_tmp, config_path) = temp_config_path();
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        None,
        None,
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    let result = handle_default_profile(&config_path, "nonexistent".to_string()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn test_config_show_json() {
    let (_tmp, config_path) = temp_config_path();
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        Some(CliAuthMethod::Basic),
        Some("admin".to_string()),
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    // Should not error
    let result = handle_show(&config_path, "dev", &OutputFormat::Json).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_config_show_csv() {
    let (_tmp, config_path) = temp_config_path();
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        Some(CliAuthMethod::Basic),
        Some("admin".to_string()),
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    let result = handle_show(&config_path, "dev", &OutputFormat::Csv).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_config_list_profiles_empty() {
    let (_tmp, config_path) = temp_config_path();
    let result = handle_list_profiles(&config_path, &OutputFormat::Json).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No profiles"));
}

#[tokio::test]
async fn test_config_list_profiles() {
    let (_tmp, config_path) = temp_config_path();
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        None,
        None,
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    handle_set_profile(
        &config_path,
        "prod".to_string(),
        Some("https://prod.service-now.com".to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    // Should not error
    let result = handle_list_profiles(&config_path, &OutputFormat::Json).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_config_delete_profile_non_default() {
    let (_tmp, config_path) = temp_config_path();
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        None,
        None,
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    handle_set_profile(
        &config_path,
        "prod".to_string(),
        Some("https://prod.service-now.com".to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    handle_delete_profile(&config_path, "prod".to_string(), false, None)
        .await
        .unwrap();

    let config = AppConfig::load_from(&config_path).unwrap();
    assert!(!config.profiles.contains_key("prod"));
    assert_eq!(config.default_profile, "dev");
}

#[tokio::test]
async fn test_config_delete_default_requires_confirmation() {
    let (_tmp, config_path) = temp_config_path();
    handle_init(
        &config_path,
        Some("https://dev.service-now.com".to_string()),
        None,
        None,
        None,
        "dev".to_string(),
    )
    .await
    .unwrap();

    let result = handle_delete_profile(&config_path, "dev".to_string(), false, None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("current default"));
}
