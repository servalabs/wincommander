import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const readWorkflow = (path: string) => readFileSync(path, "utf8").replace(/\r\n/g, "\n");
const releaseWorkflow = readWorkflow(".github/workflows/release.yml");
const prepareWorkflow = readWorkflow(".github/workflows/prepare-release.yml");
const publishTagWorkflow = readWorkflow(".github/workflows/publish-release-tag.yml");
const workflows = [releaseWorkflow, prepareWorkflow, publishTagWorkflow];

const parseWorkflow = (source: string) => Bun.YAML.parse(source) as Record<string, any>;
const parsedRelease = parseWorkflow(releaseWorkflow);
const parsedPrepare = parseWorkflow(prepareWorkflow);
const parsedPublishTag = parseWorkflow(publishTagWorkflow);

function extractMarkedNodeScript(source: string, marker: string): string {
  const blocks = [...source.matchAll(/node <<'NODE'\n([\s\S]*?)\n\s*NODE/g)].map((match) =>
    match[1].replace(/^ {10}/gm, ""),
  );
  const script = blocks.find((candidate) => candidate.includes(marker));
  if (!script) throw new Error(`Could not find ${marker}`);
  return script;
}

function runNodeScript(script: string, env: Record<string, string>) {
  return Bun.spawnSync({
    cmd: [process.execPath, "-e", script],
    env: { ...process.env, ...env },
    stdout: "pipe",
    stderr: "pipe",
  });
}

const secureBranchRules = [{
  type: "pull_request",
  parameters: {
    required_approving_review_count: 1,
    dismiss_stale_reviews_on_push: true,
    require_last_push_approval: false,
  },
}];
const secureEnvironment = {
  can_admins_bypass: false,
  protection_rules: [{
    type: "required_reviewers",
    prevent_self_review: true,
    reviewers: [{ type: "User", reviewer: { login: "reviewer" } }],
  }],
};
const secureTagRulesets = [{
  target: "tag",
  enforcement: "active",
  conditions: { ref_name: { include: ["refs/tags/v*"], exclude: [] } },
  rules: [{ type: "creation" }, { type: "deletion" }, { type: "update" }],
  current_user_can_bypass: "always",
}];

function controlEnv(overrides: Record<string, unknown> = {}) {
  const values = {
    BRANCH_RULES_JSON: secureBranchRules,
    RELEASE_ENVIRONMENT_JSON: secureEnvironment,
    TAG_RULESETS_JSON_LINES: secureTagRulesets,
    ...overrides,
  };
  return {
    BRANCH_RULES_JSON: JSON.stringify(values.BRANCH_RULES_JSON),
    RELEASE_ENVIRONMENT_JSON: JSON.stringify(values.RELEASE_ENVIRONMENT_JSON),
    TAG_RULESETS_JSON_LINES: (values.TAG_RULESETS_JSON_LINES as unknown[]).map(JSON.stringify).join("\n"),
  };
}

describe("Free release workflow controls", () => {
  test("all workflow files parse as YAML and grant permissions only to the jobs that need them", () => {
    expect(parsedPrepare.permissions).toEqual({});
    expect(parsedPrepare.jobs["prepare-release-pr"].permissions).toEqual({ contents: "write", "pull-requests": "write" });
    expect(parsedPublishTag.permissions).toEqual({});
    expect(parsedPublishTag.jobs["create-release-tag"].permissions).toEqual({ contents: "write" });
    expect(parsedRelease.permissions).toEqual({});
    expect(parsedRelease.jobs["verify-release-controls"].permissions).toEqual({ contents: "read" });
    expect(parsedRelease.jobs["align-version"].permissions).toEqual({ contents: "read" });
    expect(parsedRelease.jobs["build-free-release"].permissions).toEqual({
      contents: "write",
      "id-token": "write",
      attestations: "write",
    });
  });

  test("never self-approves, bypass-merges, or writes a version repair to main", () => {
    expect(releaseWorkflow).not.toContain("auto-approve-release");
    expect(releaseWorkflow).not.toContain("pending_deployments");
    expect(releaseWorkflow).not.toContain('state: "approved"');
    expect(prepareWorkflow).toContain("gh pr create");
    expect(prepareWorkflow).not.toContain("gh pr merge");
    expect(prepareWorkflow).not.toContain("--admin");
    expect(releaseWorkflow).not.toContain("git push origin HEAD:main");
    expect(releaseWorkflow).not.toContain("git commit -m");
  });

  test("serializes every public release that writes shared latest pointers", () => {
    expect(parsedRelease.concurrency.group).toBe("public-release");
    expect(parsedRelease.concurrency["cancel-in-progress"]).toBe(false);
  });

  test("the actual control evaluators accept a fully protected configuration", () => {
    for (const workflow of workflows) {
      const result = runNodeScript(extractMarkedNodeScript(workflow, "RELEASE_CONTROL_EVALUATOR"), controlEnv());
      expect(result.exitCode).toBe(0);
    }
  });

  test.each([
    ["stale approvals remain valid", { BRANCH_RULES_JSON: [{ ...secureBranchRules[0], parameters: { ...secureBranchRules[0].parameters, dismiss_stale_reviews_on_push: false } }] }],
    ["environment self-review is allowed", { RELEASE_ENVIRONMENT_JSON: { ...secureEnvironment, protection_rules: [{ ...secureEnvironment.protection_rules[0], prevent_self_review: false }] } }],
    ["administrators bypass the environment", { RELEASE_ENVIRONMENT_JSON: { ...secureEnvironment, can_admins_bypass: true } }],
    ["tag include pattern is empty", { TAG_RULESETS_JSON_LINES: [{ ...secureTagRulesets[0], conditions: { ref_name: { include: [], exclude: [] } } }] }],
    ["tag creation restriction is missing", { TAG_RULESETS_JSON_LINES: [{ ...secureTagRulesets[0], rules: [{ type: "deletion" }, { type: "update" }] }] }],
    ["automation cannot bypass protected tag creation", { TAG_RULESETS_JSON_LINES: [{ ...secureTagRulesets[0], current_user_can_bypass: "never" }] }],
  ])("the actual control evaluators fail closed when %s", (_name, override) => {
    for (const workflow of workflows) {
      const result = runNodeScript(extractMarkedNodeScript(workflow, "RELEASE_CONTROL_EVALUATOR"), controlEnv(override));
      expect(result.exitCode).not.toBe(0);
    }
  });

  test("last-push approval is accepted as the stale-review alternative", () => {
    const branchRules = [{
      ...secureBranchRules[0],
      parameters: { ...secureBranchRules[0].parameters, dismiss_stale_reviews_on_push: false, require_last_push_approval: true },
    }];
    for (const workflow of workflows) {
      const result = runNodeScript(extractMarkedNodeScript(workflow, "RELEASE_CONTROL_EVALUATOR"), controlEnv({ BRANCH_RULES_JSON: branchRules }));
      expect(result.exitCode).toBe(0);
    }
  });

  test("tag creation validates Cargo.lock before pushing and only follows a merged release PR", () => {
    expect(publishTagWorkflow).toContain("github.event.pull_request.merged == true");
    expect(publishTagWorkflow).toContain("startsWith(github.event.pull_request.head.ref, 'release/v')");
    expect(publishTagWorkflow).toContain('fs.readFileSync("src-tauri/Cargo.lock", "utf8")');
    expect(publishTagWorkflow.indexOf("Cargo.lock commander-free version")).toBeLessThan(publishTagWorkflow.indexOf('git push origin "v${version}"'));
  });

  test.each([
    ["newer version", { TARGET_VERSION: "3.5.3", MAIN_VERSION: "3.5.3", PUBLISHED_TAGS_LINES: "v3.5.1\nv3.5.2" }, 0],
    ["newer prerelease", { TARGET_VERSION: "3.6.0-beta.1", MAIN_VERSION: "3.6.0-beta.1", PUBLISHED_TAGS_LINES: "v3.5.2" }, 0],
    ["same published version", { TARGET_VERSION: "3.5.2", MAIN_VERSION: "3.5.2", PUBLISHED_TAGS_LINES: "v3.5.1\nv3.5.2" }, 1],
    ["older than a published prerelease", { TARGET_VERSION: "3.6.0-beta.1", MAIN_VERSION: "3.6.0-beta.1", PUBLISHED_TAGS_LINES: "v3.5.2\nv3.6.0-beta.2" }, 1],
    ["not current main", { TARGET_VERSION: "3.5.3", MAIN_VERSION: "3.5.4", PUBLISHED_TAGS_LINES: "v3.5.2" }, 1],
    ["missing publication baseline", { TARGET_VERSION: "3.5.3", MAIN_VERSION: "3.5.3", PUBLISHED_TAGS_LINES: "" }, 1],
  ])("version evaluator handles %s", (_name, env, expectedFailure) => {
    const script = extractMarkedNodeScript(publishTagWorkflow, "RELEASE_VERSION_EVALUATOR");
    const result = runNodeScript(script, env);
    expect(result.exitCode === 0 ? 0 : 1).toBe(expectedFailure);
  });
});
