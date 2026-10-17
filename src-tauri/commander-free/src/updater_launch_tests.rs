use super::preserve_app_until_installer_launch;
use std::time::Duration;
use tauri::{
    test::{mock_builder, mock_context, noop_assets},
    Manager, Resource,
};
use tauri_plugin_updater::UpdaterExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Public test-only minisign key and signature over the two-byte `MZ` fixture
// below, signed for version 99.0.0. The updater now verifies bytes during
// install, so the fixture must pass signature validation to exercise the
// Windows installer-launch failure path.
const TEST_UPDATER_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhCQUU5OERFNUREMkMwNjgKUldSb3dOSmQzcGl1aTZodUh5bTJBcmpJVVNVTVIza2QwbktYajdaZHZPdGpoVUkxWktNMnVZV3oK";
const TEST_UPDATE_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSb3dOSmQzcGl1aTUrYjdTYkVqQXMxNkRsV3FmTktBZk1FVWE0OW1NclZxVm5EdDR4a1RqWm5TdzQzL2hFWlg5UUlzTUVGVDFSdkZ3TUpYV1grTmIwNmdPYXBYMHZMMmdVPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyMTM3ODI0CWZpbGU6cGF5bG9hZC5leGUJdmVyc2lvbjo5OS4wLjAKOEJCMDJyVmN2Zjl3ZFVteFRBMjZpSWdpU3RnMFNTZmNnR1BTK2RyWGVpYVNlSTdYRGErdjcvZUZ2SEZpL05icnFLUHd0dDl6ZVpjenBjWmNKU0lTREE9PQo=";

struct LiveUiResource;
impl Resource for LiveUiResource {}

#[link(name = "kernel32")]
extern "system" {
    fn SetThreadErrorMode(mode: u32, previous: *mut u32) -> i32;
}

struct SilentLoaderErrors(u32);

impl SilentLoaderErrors {
    fn enter() -> Self {
        let mut previous = 0;
        // Keep the deliberately invalid image from opening a Windows error dialog.
        assert_ne!(unsafe { SetThreadErrorMode(0x8001, &mut previous) }, 0);
        Self(previous)
    }
}

impl Drop for SilentLoaderErrors {
    fn drop(&mut self) {
        unsafe { SetThreadErrorMode(self.0, std::ptr::null_mut()) };
    }
}

#[tokio::test]
async fn installer_launch_failure_keeps_app_resources_available_for_retry() {
    for preserve_resources in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/manifest.json", listener.local_addr().unwrap());
        let manifest = serde_json::json!({
            "version": "99.0.0",
            "url": "http://127.0.0.1/unused.exe",
            "signature": TEST_UPDATE_SIGNATURE,
        })
        .to_string();
        let server = tokio::spawn(async move {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = [0u8; 4096];
            stream.read(&mut request).await.unwrap();
            stream.write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                manifest.len(), manifest,
            ).as_bytes()).await.unwrap();
        });

        let mut context = mock_context(noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "pubkey": TEST_UPDATER_PUBLIC_KEY,
                "requireSignedVersion": true,
                "dangerousInsecureTransportProtocol": true,
            }),
        );
        let app = mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let resource = app.resources_table().add(LiveUiResource);
        let builder = app.updater_builder();
        let builder = if preserve_resources {
            preserve_app_until_installer_launch(builder)
        } else {
            builder
        };
        let update = builder
            .endpoints(vec![endpoint.parse().unwrap()])
            .unwrap()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap()
            .check()
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();

        // MZ is enough for the plugin's format detector, but contains no executable code.
        let _silent_errors = SilentLoaderErrors::enter();
        let result = update.install(b"MZ");
        assert!(
            matches!(result, Err(tauri_plugin_updater::Error::Io(_))),
            "must reach the real Windows installer-launch failure: {result:?}"
        );
        assert_eq!(app.resources_table().get::<LiveUiResource>(resource).is_ok(),
            preserve_resources,
            "the default hook destroys resources before a failed launch; the guarded hook must retain them");
    }
}
