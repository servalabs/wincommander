import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { summarizeStartupSamples, validateStartupSample, type StartupSample } from "../src/dev/startupMeasurement";

const [inputPath, outputPath] = process.argv.slice(2);
if (!inputPath) {
  throw new Error("Usage: bun tools/startup-benchmark.ts <samples.json> [report.json]");
}

const samples = JSON.parse(await readFile(resolve(inputPath), "utf8")) as StartupSample[];
const failures = samples.flatMap((sample, index) => validateStartupSample(sample).map((failure) => `sample ${index}: ${failure}`));
if (failures.length) throw new Error(`Invalid privacy-safe startup samples:\n${failures.join("\n")}`);

const report = {
  reportOnly: true,
  measuredAt: new Date().toISOString(),
  samples: samples.length,
  summaries: summarizeStartupSamples(samples),
  externalGates: ["packaged Windows", "WPR/WPA trace", "reboot", "RDS logon", "Defender/driver/physical-device acceptance"],
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await writeFile(resolve(outputPath), serialized, "utf8");
else process.stdout.write(serialized);
