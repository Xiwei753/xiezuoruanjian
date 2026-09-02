use super::*;

#[test]
fn test_github_api_error_404_get_ref_classified_as_repo_not_found() {
    let err = github_api_error("get ref heads/main", 404, "{}".to_string());
    assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
}

#[test]
fn test_github_api_error_404_get_recursive_tree_classified_as_repo_not_found() {
    let err = github_api_error("get recursive tree", 404, "{}".to_string());
    assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
}

#[test]
fn test_github_api_error_404_get_contents_classified_as_file_not_found() {
    let err = github_api_error("get contents chapter.md", 404, "{}".to_string());
    assert_eq!(err.sync_category(), "file_not_found");
}

#[test]
fn test_github_api_error_404_put_contents_classified_as_repo_not_found() {
    let err = github_api_error("put contents chapter.md", 404, "{}".to_string());
    assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
}

#[test]
fn test_github_api_error_404_delete_contents_classified_as_repo_not_found() {
    let err = github_api_error("delete contents chapter.md", 404, "{}".to_string());
    assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
}

#[test]
fn test_github_api_error_401_classified_as_token_invalid() {
    let err = github_api_error("get ref heads/main", 401, "{}".to_string());
    assert_eq!(err.sync_category(), "token_invalid");
}

#[test]
fn test_github_api_error_403_with_permission_denied_body() {
    let err = github_api_error(
        "get ref heads/main",
        403,
        "Resource not accessible by personal access token".to_string(),
    );
    assert_eq!(err.sync_category(), "token_permission_denied");
}

#[test]
fn test_github_api_error_403_without_permission_denied_body() {
    let err = github_api_error("get ref heads/main", 403, "{}".to_string());
    assert_eq!(err.sync_category(), "auth_error");
}

#[test]
fn test_github_api_error_404_generic_context_classified_as_repo_not_found() {
    let err = github_api_error("some unknown operation", 404, "{}".to_string());
    assert_eq!(err.sync_category(), "repo_not_found_or_no_permission");
}

#[test]
fn test_github_api_error_404_not_found_category_not_used() {
    let contexts = [
        "get ref heads/main",
        "get recursive tree",
        "get contents chapter.md",
        "put contents chapter.md",
        "delete contents chapter.md",
        "some unknown operation",
    ];
    for ctx in &contexts {
        let err = github_api_error(ctx, 404, "{}".to_string());
        let category = err.sync_category();
        assert!(
            category != "not_found",
            "404 for '{}' must not produce generic 'not_found' category, got: {}",
            ctx,
            category
        );
    }
}

#[test]
fn test_github_api_error_5xx_classified_as_network_error() {
    let err = github_api_error("get ref", 500, "internal server error".to_string());
    assert_eq!(err.sync_category(), "network_error");
    let err2 = github_api_error("get ref", 503, "service unavailable".to_string());
    assert_eq!(err2.sync_category(), "network_error");
}

#[test]
fn test_github_api_error_409_classified_as_remote_sha_conflict() {
    let err = github_api_error("put contents test.md", 409, "sha conflict".to_string());
    assert_eq!(err.sync_category(), "remote_sha_conflict");
}

#[test]
fn test_github_api_error_429_classified_as_api_rate_limited() {
    let err = github_api_error("get ref", 429, "rate limited".to_string());
    assert_eq!(err.sync_category(), "api_rate_limited");
}
