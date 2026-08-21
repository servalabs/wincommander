import { describe, expect, it } from "bun:test";
import { fileSearchDiagnostic } from "./fileSearchDiagnostics";
describe("file search diagnostic stages", () => {
  for (const stage of ["service_unavailable", "index_unavailable", "query_accepted", "scope_applied", "result_incomplete", "timeout", "cancelled"] as const) {
    it(`${stage} is typed and neutral`, () => {
      const diagnostic = fileSearchDiagnostic(stage);
      expect(diagnostic.stage).toBe(stage);
      expect(/[A-Z]:\\|powershell|token|clipboard/i.test(diagnostic.message)).toBe(false);
    });
  }
});
