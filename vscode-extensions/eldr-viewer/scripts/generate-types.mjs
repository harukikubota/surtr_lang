import { writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { compile } from "json-schema-to-typescript";

const __dirname = dirname(fileURLToPath(import.meta.url));
const extensionRoot = join(__dirname, "..");
const repoRoot = join(extensionRoot, "..", "..");
const schemaPath = join(extensionRoot, "schema", "viewer.schema.json");
const typesPath = join(extensionRoot, "src", "generated", "viewer.ts");

const schemaText = execFileSync(
  "cargo",
  [
    "run",
    "-p",
    "sindr",
    "--features",
    "viewer-schema",
    "--example",
    "export_viewer_schema",
    "--quiet"
  ],
  {
    cwd: repoRoot,
    encoding: "utf8"
  }
);

await writeFile(schemaPath, schemaText, "utf8");

const typeText = await compile(JSON.parse(schemaText), "ViewerFile", {
  bannerComment: "/* eslint-disable */\n/* generated from Rust viewer schema */"
});
await writeFile(typesPath, typeText, "utf8");
