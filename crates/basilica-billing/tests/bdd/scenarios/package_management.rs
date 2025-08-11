use crate::bdd::TestContext;
use basilica_protocol::billing::{GetBillingPackagesRequest, SetUserPackageRequest};

#[tokio::test]
async fn test_get_billing_packages_returns_all_available_packages() {
    let mut context = TestContext::new().await;
    let user_id = "test_user_packages_001";

    context.create_test_user(user_id, "1000.0").await;

    let request = GetBillingPackagesRequest {
        user_id: user_id.to_string(),
    };

    let response = context
        .client
        .get_billing_packages(request)
        .await
        .expect("Failed to get billing packages")
        .into_inner();

    assert!(!response.packages.is_empty(), "Should return packages");
    assert!(
        response.packages.len() >= 4,
        "Should have at least 4 packages"
    );

    let package_ids: Vec<String> = response
        .packages
        .iter()
        .map(|p| p.package_id.clone())
        .collect();

    assert!(
        package_ids.contains(&"h100".to_string()),
        "Should contain h100 package"
    );
    assert!(
        package_ids.contains(&"a100".to_string()),
        "Should contain a100 package"
    );
    assert!(
        package_ids.contains(&"rtx4090".to_string()),
        "Should contain rtx4090 package"
    );
    assert!(
        package_ids.contains(&"custom".to_string()),
        "Should contain custom package"
    );

    for package in &response.packages {
        assert!(
            !package.package_id.is_empty(),
            "Package ID should not be empty"
        );
        assert!(!package.name.is_empty(), "Package name should not be empty");
        assert!(package.rates.is_some(), "Package should have rates");
    }

    context.cleanup().await;
}

#[tokio::test]
async fn test_get_billing_packages_returns_current_user_package() {
    let mut context = TestContext::new().await;
    let user_id = "test_user_current_package";

    context.create_test_user(user_id, "1000.0").await;

    let set_request = SetUserPackageRequest {
        user_id: user_id.to_string(),
        package_id: "a100".to_string(),
        effective_from: None,
    };

    let _set_response = context
        .client
        .set_user_package(set_request)
        .await
        .expect("Failed to set user package");

    let get_request = GetBillingPackagesRequest {
        user_id: user_id.to_string(),
    };

    let response = context
        .client
        .get_billing_packages(get_request)
        .await
        .expect("Failed to get billing packages")
        .into_inner();

    assert_eq!(
        response.current_package_id, "a100",
        "Current package should be a100"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_set_user_package_updates_user_preference() {
    let mut context = TestContext::new().await;
    let user_id = "test_set_package_001";

    context.create_test_user(user_id, "1000.0").await;

    let initial_package = context.get_user_package(user_id).await;
    assert!(
        initial_package.is_none(),
        "User should not have a package preference initially"
    );

    let request = SetUserPackageRequest {
        user_id: user_id.to_string(),
        package_id: "rtx4090".to_string(),
        effective_from: None,
    };

    let response = context
        .client
        .set_user_package(request)
        .await
        .expect("Failed to set user package")
        .into_inner();

    assert!(response.success, "Setting package should succeed");
    assert_eq!(
        response.new_package_id, "rtx4090",
        "New package should be rtx4090"
    );

    let updated_package = context.get_user_package(user_id).await;
    assert_eq!(
        updated_package,
        Some("rtx4090".to_string()),
        "Package should be updated in database"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_set_user_package_tracks_previous_package() {
    let mut context = TestContext::new().await;
    let user_id = "test_package_history";

    context.create_test_user(user_id, "1000.0").await;

    let first_request = SetUserPackageRequest {
        user_id: user_id.to_string(),
        package_id: "h100".to_string(),
        effective_from: None,
    };

    let first_response = context
        .client
        .set_user_package(first_request)
        .await
        .expect("Failed to set first package")
        .into_inner();

    assert!(first_response.success);
    assert_eq!(first_response.new_package_id, "h100");

    let second_request = SetUserPackageRequest {
        user_id: user_id.to_string(),
        package_id: "a100".to_string(),
        effective_from: None,
    };

    let second_response = context
        .client
        .set_user_package(second_request)
        .await
        .expect("Failed to set second package")
        .into_inner();

    assert!(second_response.success);
    assert_eq!(
        second_response.previous_package_id, "h100",
        "Should track previous package"
    );
    assert_eq!(
        second_response.new_package_id, "a100",
        "Should update to new package"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_set_invalid_package_returns_error() {
    let mut context = TestContext::new().await;
    let user_id = "test_invalid_package";

    context.create_test_user(user_id, "1000.0").await;

    let request = SetUserPackageRequest {
        user_id: user_id.to_string(),
        package_id: "nonexistent_package".to_string(),
        effective_from: None,
    };

    let result = context.client.set_user_package(request).await;

    assert!(result.is_err(), "Setting invalid package should fail");

    let package_after = context.get_user_package(user_id).await;
    assert!(
        package_after.is_none(),
        "Package should not be set after failed attempt"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_package_effective_date_is_respected() {
    let mut context = TestContext::new().await;
    let user_id = "test_effective_date";

    context.create_test_user(user_id, "1000.0").await;

    let future_time = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    let timestamp = prost_types::Timestamp::from(future_time);

    let request = SetUserPackageRequest {
        user_id: user_id.to_string(),
        package_id: "custom".to_string(),
        effective_from: Some(timestamp.clone()),
    };

    let response = context
        .client
        .set_user_package(request)
        .await
        .expect("Failed to set package with effective date")
        .into_inner();

    assert!(response.success);
    assert_eq!(response.new_package_id, "custom");
    assert_eq!(response.effective_from, Some(timestamp));

    let db_package = context.get_user_package(user_id).await;
    assert_eq!(db_package, Some("custom".to_string()));

    context.cleanup().await;
}
