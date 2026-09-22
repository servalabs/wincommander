import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { createStartupBenchmarkReport } from "../src/dev/startupBenchmark";

try {
  const args = process.argv.slice(2);
  const requireComplete = args.includes("--require-complete");
  const paths = args.filter((arg) => arg !== "--require-complete");
  if (paths.length < 1 || paths.length > 2 || paths.some((path) => path.startsWith("--"))) {
    throw new Error("Usage: bun tools/startup-benchmark.ts <samples.json> [report.json] [--require-complete]");
  }
  const [inputPath, outputPath] = paths;
  const normalized = (path: string) => process.platform === "win32" ? resolve(path).toLowerCase() : resolve(path);
  if (outputPath && normalized(inputPath) === normalized(outputPath)) throw new Error("The report must not overwrite the raw sample input.");
  const input: unknown = JSON.parse(await readFile(resolve(inputPath), "utf8"));
  const report = { ...createStartupBenchmarkReport(input), generatedAt: new Date().toISOString() };
  const serialized = `${JSON.stringify(report, null, 2)}\n`;
  if (outputPath) await writeFile(resolve(outputPath), serialized, { encoding: "utf8", flag: "wx" });
  else process.stdout.write(serialized);
  if (report.rejectedSamples > 0 || (requireComplete && !report.dataComplete)) {
    process.stderr.write("Startup samples did not pass the requested validation; inspect the report issues.\n");
    process.exitCode = 1;
  }
} catch (error) {
  process.stderr.write(`${error instanceof SyntaxError ? "Invalid startup sample JSON." : error instanceof Error ? error.message : "Startup sample processing failed."}\n`);
  process.exitCode = 1;
}
