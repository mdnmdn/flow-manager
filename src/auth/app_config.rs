// Compile-time values injected via CI build secrets
pub const COMPILED_CLIENT_ID: Option<&str> = option_env!("GITHUB_CLIENT_ID");
pub const COMPILED_APP_ID: Option<&str> = option_env!("GITHUB_APP_ID");

// Returns client ID: compile-time baked value first, then runtime env var
pub fn client_id() -> Option<String> {
    COMPILED_CLIENT_ID
        .map(|s| s.to_string())
        .or_else(|| std::env::var("GITHUB_CLIENT_ID").ok())
}

// Returns app ID: compile-time baked value first, then runtime env var
pub fn app_id() -> Option<String> {
    COMPILED_APP_ID
        .map(|s| s.to_string())
        .or_else(|| std::env::var("GITHUB_APP_ID").ok())
}
