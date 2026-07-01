#!/usr/bin/env node

const { run } = require("../scripts/run");

run("mimofan").catch((error) => {
  console.error("Failed to start mimofan:", error.message);
  process.exit(1);
});
