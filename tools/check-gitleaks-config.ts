import { strict as assert } from "node:assert";
import { randomBytes } from "node:crypto";
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

const scanner = process.env.GITLEAKS_BIN ?? Bun.which("gitleaks");
const config = process.env.GITLEAKS_TEST_CONFIG ?? resolve(import.meta.dirname, "../.gitleaks.toml");

function scanFixture(contents: string) {
    if (!scanner) throw new Error("Install Gitleaks or provide GITLEAKS_BIN to verify scanner behavior.");
    const temporaryRoot = realpathSync(tmpdir());
    const directory = mkdtempSync(join(temporaryRoot, "wincmd-gitleaks-test-"));
    const report = join(directory, "result.json");
    try {
        const source = join(directory, "fixture");
        writeFileSync(source, contents);
        const env = Object.fromEntries(Object.entries(process.env).filter(([key]) =>
            ["PATH", "SYSTEMROOT", "WINDIR", "TEMP", "TMP"].includes(key.toUpperCase())));
        const result = Bun.spawnSync([
            scanner, "dir", source, "--config", config, "--redact=100", "--no-banner",
            "--log-level", "error", "--report-format", "json", "--report-path", report,
        ], { env, stdout: "pipe", stderr: "pipe", timeout: 30_000 });
        const findings = JSON.parse(readFileSync(report, "utf8")) as { RuleID: string }[];
        return { exit: result.exitCode, rules: findings.map((entry) => entry.RuleID) };
    } finally {
        if (dirname(directory) !== temporaryRoot) throw new Error("Unexpected scanner fixture path.");
        rmSync(directory, { recursive: true, force: true });
    }
}

{
    const generatedControl = ["gh", "p_"].join("") + randomBytes(24).toString("hex").slice(0, 36);
    const result = scanFixture(`assessment_control = "${generatedControl}"`);
    assert.equal(result.exit, 1, "The repository configuration must detect the generated positive control.");
    assert.ok(result.rules.includes("github-pat"), "The built-in GitHub rule must be active.");
}

{
    const result = scanFixture("assessment_control = no credential material\n");
    assert.equal(result.exit, 0, "Ordinary text must remain clean.");
    assert.deepEqual(result.rules, []);
}

console.log("Repository Gitleaks configuration: positive and negative controls passed.");
