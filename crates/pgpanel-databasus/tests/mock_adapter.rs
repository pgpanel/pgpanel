use pgpanel_databasus::{build_mock_adapter, DatabasusAdapter, RegisterRequest};
use uuid::Uuid;

#[tokio::test]
async fn mock_register_and_status() {
    let adapter = build_mock_adapter();
    assert!(adapter.test_connection().await.unwrap());

    let reg = adapter
        .register_cluster(RegisterRequest {
            cluster_id: Uuid::new_v4(),
            name: "test".into(),
            host: "pgpanel_pg_test".into(),
            port: 5432,
            username: "pgpanel_backup".into(),
            password: "not-logged".into(),
            database: "postgres".into(),
            postgres_version: "17".into(),
        })
        .await
        .unwrap();

    assert!(reg.external_id.is_some());
    let status = adapter
        .get_backup_status(reg.external_id.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(status.failed_backups, 0);
}

#[tokio::test]
async fn mock_can_fail_registration() {
    use pgpanel_databasus::MockDatabasusAdapter;
    let mock = MockDatabasusAdapter::new();
    mock.set_fail_next(true);
    let err = mock
        .register_cluster(RegisterRequest {
            cluster_id: Uuid::new_v4(),
            name: "x".into(),
            host: "h".into(),
            port: 5432,
            username: "u".into(),
            password: "p".into(),
            database: "postgres".into(),
            postgres_version: "17".into(),
        })
        .await;
    assert!(err.is_err());
}
