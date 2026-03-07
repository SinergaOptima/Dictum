import { spawn } from "node:child_process";

const RETRYABLE_PATTERNS = [
  "TS6053",
  ".next/types/app/layout.ts",
  ".next/types/app/page.ts",
  ".next/types/app/pill/page.ts",
];

function runTsc() {
  return new Promise((resolve) => {
    const child = spawn(
      process.execPath,
      ["./node_modules/typescript/bin/tsc", "--noEmit"],
      {
        cwd: process.cwd(),
        stdio: ["ignore", "pipe", "pipe"],
      },
    );

    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      const text = chunk.toString();
      stdout += text;
      process.stdout.write(text);
    });
    child.stderr.on("data", (chunk) => {
      const text = chunk.toString();
      stderr += text;
      process.stderr.write(text);
    });
    child.on("close", (code) => {
      resolve({
        code: code ?? 1,
        output: `${stdout}\n${stderr}`,
      });
    });
  });
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function isRetryable(output) {
  return RETRYABLE_PATTERNS.some((pattern) => output.includes(pattern));
}

let lastCode = 1;
for (let attempt = 1; attempt <= 3; attempt += 1) {
  const result = await runTsc();
  lastCode = result.code;
  if (result.code === 0) {
    process.exit(0);
  }
  if (attempt === 3 || !isRetryable(result.output)) {
    process.exit(result.code);
  }
  await sleep(1200);
}

process.exit(lastCode);
