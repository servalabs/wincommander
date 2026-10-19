import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src-tauri/commander-free/src/pro_install.rs", "utf8");

test("Pro update clients preserve the pinned origin without following redirects", () => {
    expect(source).toContain("fn pro_update_http_client(");
    expect(source).toContain(".redirect(reqwest::redirect::Policy::none())");
    expect(source).toContain(".dns_resolver(crate::net::doh_resolver())");
    expect(source).not.toContain(".no_proxy()");
});

test("Pro manifests are bounded before JSON decoding and binary bodies stream into staging", () => {
    const manifest = source.slice(source.indexOf("pub async fn fetch_pro_manifest("), source.indexOf("pub async fn get_pro_install_status("));
    expect(manifest).toContain("read_pro_response(resp, download_policy::MAX_MANIFEST_BYTES");
    expect(manifest).toContain("serde_json::from_slice(&bytes)");
    const download = source.slice(source.indexOf("// 2. Download to a sibling .tmp."));
    expect(download).toContain("download_policy::check_content_length");
    expect(download).toContain("while let Some(chunk) = resp");
    expect(download).toContain("download_policy::checked_body_size");
    expect(download).toContain("f.write_all(&chunk)");
    expect(download.indexOf("if actual != expected_sha256.to_lowercase()")).toBeLessThan(download.indexOf("verify_pro_package_version("));
    expect(download).not.toContain(".bytes()");
});
