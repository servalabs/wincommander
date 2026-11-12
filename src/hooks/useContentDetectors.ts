import { invoke } from "@tauri-apps/api/core";
import type { Finding, Rule } from "../types/generated/fleet";

export interface LocalContentTestResult { findings: Finding[]; truncated: boolean }

export function loadContentDetectorCatalog(): Promise<Rule[]> {
  return invoke<Rule[]>("get_content_detector_catalog");
}

export function testLocalContentRules(rules: Rule[], sample: string): Promise<LocalContentTestResult> {
  return invoke<LocalContentTestResult>("test_local_content_rules", { rules, sample });
}
