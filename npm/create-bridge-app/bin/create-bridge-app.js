#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");

const projectName = process.argv[2];
if (!projectName) {
  console.error("usage: create-bridge-app <project-dir>");
  process.exit(1);
}

const cwd = process.cwd();
const target = path.join(cwd, projectName);
if (fs.existsSync(target)) {
  console.error(`target path already exists: ${target}`);
  process.exit(1);
}

fs.mkdirSync(target, { recursive: true });
const templates = path.join(__dirname, "..", "templates");
copyRecursive(templates, target);
console.log(`initialized bridge project at ${projectName}`);

function copyRecursive(from, to) {
  const entries = fs.readdirSync(from, { withFileTypes: true });
  for (const entry of entries) {
    const srcPath = path.join(from, entry.name);
    const destPath = path.join(to, entry.name);
    if (entry.isDirectory()) {
      fs.mkdirSync(destPath, { recursive: true });
      copyRecursive(srcPath, destPath);
    } else {
      fs.copyFileSync(srcPath, destPath);
    }
  }
}
