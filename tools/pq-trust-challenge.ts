// SPDX-License-Identifier: AGPL-3.0-or-later
// Public challenge preparation only; never reads signing keys.
import { writeFileSync } from "node:fs";
import { trustProofMessage } from "./pq-release-profile";

const [purpose, output] = process.argv.slice(2);
if (!purpose || !["LIC", "FREE", "PRO"].includes(purpose) || !output || process.argv.length !== 4)
  throw new Error("Usage: bun tools/pq-trust-challenge.ts LIC|FREE|PRO <new-output-file>");
const message = trustProofMessage(purpose as "LIC" | "FREE" | "PRO", process.env);
writeFileSync(output, message, { flag: "wx" });
