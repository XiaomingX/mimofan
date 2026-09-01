#!/usr/bin/env bun

const { runMimofan } = require("../scripts/run");

runMimofan().catch((error) => {
  console.error("Failed to start mimofan:", error.message);
  process.exit(1);
});
